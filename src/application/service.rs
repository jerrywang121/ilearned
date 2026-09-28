use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::application::ranking::{cosine, rrf_fuse, RRF_K};
use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery, TopicQuery,
};
use crate::domain::experience::{Experience, State};
use crate::domain::lifecycle::LifecycleConfig;
use crate::domain::topics::{
    topic_matches, truncate_topic, validate_topic, validate_topic_pattern,
};
use crate::embedding::provider::{DynProvider, EmbeddingProvider};
use crate::error::AppError;
use crate::storage::embeddings::VectorStore;
use crate::storage::repository::ExperienceRepo;

pub const MAX_LIMIT: u32 = 100;

/// Outcome of importing one validated record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    New,
    Updated,
}

/// One skipped line during bulk import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError {
    pub line: usize,
    pub message: String,
}

/// Totals for a bulk import run. All lines are attempted; bad lines are
/// collected in `errors` while good lines are still applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub new: usize,
    pub updated: usize,
    pub errors: Vec<ImportError>,
}

/// The sole entry point for adapters. Owns lifecycle, ranking, transactions.
pub struct MemoryService<R> {
    repo: R,
    lifecycle: LifecycleConfig,
    embedding: Option<DynProvider>,
}

impl<R: ExperienceRepo + VectorStore> MemoryService<R> {
    pub fn new(repo: R, lifecycle: LifecycleConfig) -> Self {
        Self {
            repo,
            lifecycle,
            embedding: None,
        }
    }

    pub fn with_embedding_provider<P: EmbeddingProvider + 'static>(mut self, p: P) -> Self {
        self.embedding = Some(Arc::new(p));
        self
    }

    /// Test escape hatch: direct repo access.
    pub fn repo(&self) -> &R {
        &self.repo
    }

    /// Explicit `(topic, id)` lookup. Reconciles first (sole-entry rule).
    /// Returns `NotFound` for missing or `deleted` rows; `forgotten` and
    /// `inactive` remain reachable here (search still hides them).
    pub fn get(&self, topic: &str, id: &str) -> Result<Experience, AppError> {
        self.reconcile()?;
        match self.repo.get(topic, id)? {
            Some(e) if !matches!(e.state, State::Deleted) => Ok(e),
            _ => Err(AppError::NotFound {
                topic: topic.to_string(),
                id: id.to_string(),
            }),
        }
    }

    fn reconcile(&self) -> Result<(), AppError> {
        self.repo.reconcile(Utc::now(), &self.lifecycle)
    }

    /// Block on an async embed from sync code. Runs the future on a fresh
    /// current-thread runtime in a helper thread so this works both inside
    /// an axum handler runtime and in plain sync contexts (CLI, tests).
    fn block_embed(&self, text: &str) -> Result<Vec<f32>, AppError> {
        let provider = self.embedding.clone().ok_or_else(|| {
            AppError::EmbeddingUnavailable("no embedding provider configured".to_string())
        })?;
        let text = text.to_string();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| AppError::Internal(e.to_string()))?;
            rt.block_on(provider.embed(&text))
        })
        .join()
        .map_err(|_| AppError::Internal("embedding thread panicked".to_string()))?
    }

    fn doc_text(e: &Experience) -> String {
        format!(
            "{} {} {} {}",
            e.when_text, e.if_text, e.do_text, e.check_text
        )
    }

    /// Best-effort (re)embedding after a canonical write. Failures are
    /// logged; the canonical write stands.
    fn best_effort_embed(&self, e: &Experience) {
        if let Some(p) = self.embedding.clone() {
            let text = Self::doc_text(e);
            let model = p.model_id().to_string();
            let topic = e.topic.clone();
            let id = e.id.clone();
            match self.block_embed(&text) {
                Ok(v) => {
                    if let Err(err) = self.repo.upsert_vector(&topic, &id, &model, &v) {
                        eprintln!("ilearned: vector upsert failed: {err}");
                    }
                }
                Err(err) => eprintln!("ilearned: embedding failed (canonical write kept): {err}"),
            }
        }
    }

    pub fn add(&self, cmd: AddCommand) -> Result<Experience, AppError> {
        cmd.validate()?;
        self.reconcile()?;
        let now = Utc::now();
        for _ in 0..2 {
            let id = Uuid::new_v4().to_string()[..8].to_string();
            let e = Experience {
                topic: cmd.topic.clone(),
                id,
                when_text: cmd.when_text.clone(),
                if_text: cmd.if_text.clone(),
                do_text: cmd.do_text.clone(),
                check_text: cmd.check_text.clone(),
                updated_at: now,
                good_count: 1,
                bad_count: 0,
                state: State::Active,
            };
            match self.repo.insert(&e) {
                Ok(()) => {
                    self.best_effort_embed(&e);
                    return Ok(e);
                }
                // UUID-8 collision: retry once. Any other storage error is real.
                Err(AppError::Storage(m))
                    if m.contains("UNIQUE constraint failed") || m.contains("PRIMARY KEY") =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(AppError::Internal("id collision".to_string()))
    }

    fn restore(&self, mut e: Experience) -> Experience {
        if matches!(e.state, State::Inactive | State::Forgotten) {
            e.state = State::Active;
        }
        e.updated_at = Utc::now();
        e
    }

    fn visible(&self, e: Experience, deep: bool) -> Option<Experience> {
        match e.state {
            State::Deleted => None,
            State::Forgotten => None,
            State::Inactive if !deep => None,
            _ => Some(e),
        }
    }

    pub fn modify(&self, cmd: ModifyCommand) -> Result<Experience, AppError> {
        if !cmd.has_updates() {
            return Err(AppError::InvalidInput(
                "at least one of when/if/do/check is required".to_string(),
            ));
        }
        self.reconcile()?;
        let mut e = match self.repo.get(&cmd.topic, &cmd.id)? {
            Some(e) if !matches!(e.state, State::Deleted) => e,
            _ => {
                return Err(AppError::NotFound {
                    topic: cmd.topic,
                    id: cmd.id,
                });
            }
        };
        if let Some(w) = cmd.when_text.filter(|s| !s.trim().is_empty()) {
            e.when_text = w;
        }
        if let Some(i) = cmd.if_text.filter(|s| !s.trim().is_empty()) {
            e.if_text = i;
        }
        if let Some(d) = cmd.do_text.filter(|s| !s.trim().is_empty()) {
            e.do_text = d;
        }
        if let Some(c) = cmd.check_text.filter(|s| !s.trim().is_empty()) {
            e.check_text = c;
        }
        let e = self.restore(e);
        self.repo.update(&e)?;
        self.best_effort_embed(&e);
        Ok(e)
    }

    pub fn delete(&self, topic: &str, id: &str) -> Result<(), AppError> {
        self.reconcile()?;
        // Deleted rows read as missing; never-existing rows are 404 too.
        // Only already-deleted soft_delete hits stay idempotent Ok.
        if self.repo.get(topic, id)?.is_none() {
            return Err(AppError::NotFound {
                topic: topic.to_string(),
                id: id.to_string(),
            });
        }
        let _ = self.repo.soft_delete(topic, id, Utc::now())?;
        Ok(())
    }

    pub fn promote(&self, f: &FeedbackCommand) -> Result<Experience, AppError> {
        self.reconcile()?;
        let mut e = match self.repo.get(&f.topic, &f.id)? {
            Some(e) if !matches!(e.state, State::Deleted) => e,
            _ => {
                return Err(AppError::NotFound {
                    topic: f.topic.clone(),
                    id: f.id.clone(),
                });
            }
        };
        e.good_count += 1;
        let e = self.restore(e);
        self.repo.update(&e)?;
        Ok(e)
    }

    pub fn downgrade(&self, f: &FeedbackCommand) -> Result<Experience, AppError> {
        self.reconcile()?;
        let mut e = match self.repo.get(&f.topic, &f.id)? {
            Some(e) if !matches!(e.state, State::Deleted) => e,
            _ => {
                return Err(AppError::NotFound {
                    topic: f.topic.clone(),
                    id: f.id.clone(),
                });
            }
        };
        e.bad_count += 1;
        let e = self.restore(e);
        self.repo.update(&e)?;
        Ok(e)
    }

    pub fn clear(&self, cmd: &ClearCommand) -> Result<u64, AppError> {
        // Destructive path stays exact-match: `#` is pattern-only, never a target.
        if let ClearCommand::Topic(t) = cmd {
            validate_topic(t)?;
        }
        self.reconcile()?;
        self.repo.clear(cmd, Utc::now())
    }

    fn validate_import(e: &Experience) -> Result<(), AppError> {
        validate_topic(&e.topic)?;
        if e.id.trim().is_empty() {
            return Err(AppError::InvalidInput("id is required".to_string()));
        }
        if e.when_text.trim().is_empty() {
            return Err(AppError::InvalidInput("when is required".to_string()));
        }
        if e.if_text.trim().is_empty() {
            return Err(AppError::InvalidInput("if is required".to_string()));
        }
        if e.do_text.trim().is_empty() {
            return Err(AppError::InvalidInput("do is required".to_string()));
        }
        if e.check_text.trim().is_empty() {
            return Err(AppError::InvalidInput("check is required".to_string()));
        }
        Ok(())
    }

    /// Dump experiences as JSONL-ready records. Reconcile-first, same
    /// visibility filter as search (deleted/forgotten always hidden,
    /// inactive only with `deep`). The topic filter accepts `#` wildcards.
    pub fn export(&self, topic: Option<&str>, deep: bool) -> Result<Vec<Experience>, AppError> {
        self.reconcile()?;
        let (sql_topic, wildcard) = Self::resolve_topic_filter(topic)?;
        Ok(self
            .repo
            .browse(sql_topic.as_deref(), deep)?
            .into_iter()
            .filter(|e| {
                wildcard
                    .as_deref()
                    .is_none_or(|p| topic_matches(p, &e.topic))
            })
            .filter_map(|e| self.visible(e, deep))
            .collect())
    }

    /// Import one validated record. With `merge`, keeps `(topic, id)` and
    /// overwrites on collision; otherwise assigns a fresh id.
    pub fn import_record(&self, mut e: Experience, merge: bool) -> Result<ImportOutcome, AppError> {
        Self::validate_import(&e)?;
        self.reconcile()?;
        if !merge {
            e.id = Uuid::new_v4().to_string()[..8].to_string();
        }
        match self.repo.get(&e.topic, &e.id)? {
            Some(_) => {
                self.repo.update(&e)?;
                self.best_effort_embed(&e);
                Ok(ImportOutcome::Updated)
            }
            None => match self.repo.insert(&e) {
                Ok(()) => {
                    self.best_effort_embed(&e);
                    Ok(ImportOutcome::New)
                }
                // UUID-8 collision on fresh ids: retry once. Any other
                // storage error is real.
                Err(AppError::Storage(m))
                    if !merge
                        && (m.contains("UNIQUE constraint failed")
                            || m.contains("PRIMARY KEY")) =>
                {
                    e.id = Uuid::new_v4().to_string()[..8].to_string();
                    self.repo.insert(&e)?;
                    self.best_effort_embed(&e);
                    Ok(ImportOutcome::New)
                }
                Err(e) => Err(e),
            },
        }
    }

    /// Bulk import: parse every JSONL line, apply the good ones, collect
    /// per-line errors. Never aborts early — good lines commit even when
    /// other lines fail.
    pub fn import_jsonl(&self, text: &str, merge: bool) -> Result<ImportSummary, AppError> {
        let mut summary = ImportSummary::default();
        for (idx, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let n = idx + 1;
            let record: Experience = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(e) => {
                    summary.errors.push(ImportError {
                        line: n,
                        message: format!("invalid JSON: {e}"),
                    });
                    continue;
                }
            };
            match self.import_record(record, merge) {
                Ok(ImportOutcome::New) => summary.new += 1,
                Ok(ImportOutcome::Updated) => summary.updated += 1,
                Err(e) => summary.errors.push(ImportError {
                    line: n,
                    message: e.to_string(),
                }),
            }
        }
        Ok(summary)
    }

    fn paginate(&self, mut out: Vec<Experience>, q: &SearchQuery) -> Vec<Experience> {
        let limit = q.limit.min(MAX_LIMIT) as usize;
        let offset = q.offset as usize;
        if offset >= out.len() || limit == 0 {
            return vec![];
        }
        out.truncate(offset + limit);
        out[offset..].to_vec()
    }

    /// Resolve a `search`/`export` topic filter: validate as a pattern and
    /// split it into an exact SQL topic plus an optional wildcard pattern.
    /// Returns `(sql_topic, wildcard)`: exact patterns keep the fast
    /// `topic=?` path; `#` patterns fetch the superset and filter in Rust.
    fn resolve_topic_filter(
        topic: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), AppError> {
        match topic {
            None => Ok((None, None)),
            Some(t) => {
                validate_topic_pattern(t)?;
                if t.split('/').any(|s| s == "#") {
                    Ok((None, Some(t.to_string())))
                } else {
                    Ok((Some(t.to_string()), None))
                }
            }
        }
    }

    /// Semantic-only search: cosine over stored vectors of eligible records.
    /// `sql_topic` is the exact SQL filter (None when a `#` wildcard
    /// applies); `wildcard` is the Rust-side pattern filter.
    fn search_semantic(
        &self,
        query_text: &str,
        q: &SearchQuery,
        sql_topic: Option<&str>,
        wildcard: Option<&str>,
    ) -> Result<Vec<Experience>, AppError> {
        let provider = self.embedding.clone().ok_or_else(|| {
            AppError::EmbeddingUnavailable("no embedding provider configured".to_string())
        })?;
        // Embed failure => typed error, never a silent text fallback.
        let qv = self.block_embed(query_text)?;
        let model = provider.model_id().to_string();
        let stored = self.repo.load_vectors(sql_topic, &model)?;
        let ids: HashSet<(String, String)> = stored
            .iter()
            .map(|(t, i, _)| (t.clone(), i.clone()))
            .collect();
        let mut scored: Vec<(Experience, f32)> = Vec::new();
        for (t, i) in ids {
            if wildcard.is_some_and(|p| !topic_matches(p, &t)) {
                continue;
            }
            if let Some(e) = self.repo.get(&t, &i)? {
                if self.visible(e.clone(), q.deep).is_none() {
                    continue;
                }
                let v = stored
                    .iter()
                    .find(|(st, si, _)| st == &t && si == &i)
                    .map(|(_, _, v)| v)
                    .expect("id came from stored");
                scored.push((e, cosine(&qv, v)));
            }
        }
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.updated_at.cmp(&a.0.updated_at))
                .then_with(|| a.0.topic.cmp(&b.0.topic))
                .then_with(|| a.0.id.cmp(&b.0.id))
        });
        Ok(scored.into_iter().map(|(e, _)| e).collect())
    }

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<Experience>, AppError> {
        self.reconcile()?;
        let (sql_topic, wildcard) = Self::resolve_topic_filter(q.topic.as_deref())?;
        let wildcard = wildcard.as_deref();
        let matches_wildcard = |e: &Experience| wildcard.is_none_or(|p| topic_matches(p, &e.topic));
        let text = q.text.as_deref().filter(|t| !t.trim().is_empty());
        let semantic = q.semantic.as_deref().filter(|s| !s.trim().is_empty());
        match (text, semantic) {
            (None, None) => {
                let out: Vec<Experience> = self
                    .repo
                    .browse(sql_topic.as_deref(), q.deep)?
                    .into_iter()
                    .filter(|e| matches_wildcard(e))
                    .filter_map(|e| self.visible(e, q.deep))
                    .collect();
                Ok(self.paginate(out, q))
            }
            (Some(t), None) => {
                let out: Vec<Experience> = self
                    .repo
                    .search_fts(t, sql_topic.as_deref(), q.deep)?
                    .into_iter()
                    .map(|(e, _)| e)
                    .filter(|e| matches_wildcard(e))
                    .filter_map(|e| self.visible(e, q.deep))
                    .collect();
                Ok(self.paginate(out, q))
            }
            (None, Some(s)) => {
                let out = self.search_semantic(s, q, sql_topic.as_deref(), wildcard)?;
                Ok(self.paginate(out, q))
            }
            (Some(t), Some(s)) => {
                // Combined: both rankings must succeed; embed failure is a
                // typed error, never a silent downgrade to text-only.
                let text_hits = self.repo.search_fts(t, sql_topic.as_deref(), q.deep)?;
                let sem_hits = self.search_semantic(s, q, sql_topic.as_deref(), wildcard)?;
                let text_keys: Vec<(String, String)> = text_hits
                    .iter()
                    .filter(|(e, _)| matches_wildcard(e))
                    .map(|(e, _)| (e.topic.clone(), e.id.clone()))
                    .collect();
                let sem_keys: Vec<(String, String)> = sem_hits
                    .iter()
                    .map(|e| (e.topic.clone(), e.id.clone()))
                    .collect();
                let fused = rrf_fuse(&text_keys, &sem_keys, RRF_K);
                let mut by_key: HashMap<(String, String), Experience> = HashMap::new();
                for (e, _) in text_hits {
                    if matches_wildcard(&e) {
                        by_key.entry((e.topic.clone(), e.id.clone())).or_insert(e);
                    }
                }
                for e in sem_hits {
                    by_key.entry((e.topic.clone(), e.id.clone())).or_insert(e);
                }
                let mut ordered: Vec<(Experience, f32)> = fused
                    .into_iter()
                    .filter_map(|(k, score)| by_key.remove(&k).map(|e| (e, score)))
                    .filter(|(e, _)| self.visible(e.clone(), q.deep).is_some())
                    .collect();
                ordered.sort_by(|a, b| {
                    b.1.partial_cmp(&a.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.0.updated_at.cmp(&a.0.updated_at))
                        .then_with(|| a.0.topic.cmp(&b.0.topic))
                        .then_with(|| a.0.id.cmp(&b.0.id))
                });
                Ok(self.paginate(ordered.into_iter().map(|(e, _)| e).collect(), q))
            }
        }
    }

    /// List/search distinct topics. Reconcile-first; visibility follows
    /// search (deleted/forgotten never contribute, inactive only with
    /// `deep`). `query` is a substring or `#` pattern matched against the
    /// full topic; `level` truncates after matching, then dedups.
    pub fn list_topics(&self, q: &TopicQuery) -> Result<Vec<String>, AppError> {
        if q.level == Some(0) {
            return Err(AppError::InvalidInput("level must be >= 1".to_string()));
        }
        self.reconcile()?;
        let base = self.repo.distinct_topics(q.deep)?;
        let query = q.query.clone().filter(|s| !s.trim().is_empty());
        // Validate a `#` pattern once up front, not per stored topic.
        let is_pattern = query
            .as_deref()
            .is_some_and(|p| p.split('/').any(|s| s == "#"));
        if let Some(ref pat) = query {
            if is_pattern {
                validate_topic_pattern(pat)?;
            }
        }
        let mut out: Vec<String> = Vec::new();
        for t in base {
            if let Some(ref pat) = query {
                if is_pattern {
                    if !topic_matches(pat, &t) {
                        continue;
                    }
                } else if !t.contains(pat.as_str()) {
                    continue;
                }
            }
            let shown = match q.level {
                Some(n) => truncate_topic(&t, n),
                None => t,
            };
            if !out.contains(&shown) {
                out.push(shown);
            }
        }
        out.sort();
        let limit = (q.limit.min(MAX_LIMIT)) as usize;
        let offset = q.offset as usize;
        if offset >= out.len() || limit == 0 {
            return Ok(vec![]);
        }
        out.truncate(offset + limit);
        Ok(out[offset..].to_vec())
    }
}
