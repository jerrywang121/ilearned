use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::application::ranking::{cosine, rrf_fuse, RRF_K};
use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery,
};
use crate::domain::experience::{Experience, State};
use crate::domain::lifecycle::LifecycleConfig;
use crate::embedding::provider::{DynProvider, EmbeddingProvider};
use crate::error::AppError;
use crate::storage::embeddings::VectorStore;
use crate::storage::repository::ExperienceRepo;

pub const MAX_LIMIT: u32 = 100;

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
                Err(AppError::Storage(_)) => continue, // PK collision: retry once
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
        if let Some(w) = cmd.when_text {
            e.when_text = w;
        }
        if let Some(i) = cmd.if_text {
            e.if_text = i;
        }
        if let Some(d) = cmd.do_text {
            e.do_text = d;
        }
        if let Some(c) = cmd.check_text {
            e.check_text = c;
        }
        let e = self.restore(e);
        self.repo.update(&e)?;
        self.best_effort_embed(&e);
        Ok(e)
    }

    pub fn delete(&self, topic: &str, id: &str) -> Result<(), AppError> {
        self.reconcile()?;
        // Idempotent: missing or already-deleted both succeed.
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
        self.reconcile()?;
        self.repo.clear(cmd, Utc::now())
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

    /// Semantic-only search: cosine over stored vectors of eligible records.
    fn search_semantic(
        &self,
        query_text: &str,
        q: &SearchQuery,
    ) -> Result<Vec<Experience>, AppError> {
        let provider = self.embedding.clone().ok_or_else(|| {
            AppError::EmbeddingUnavailable("no embedding provider configured".to_string())
        })?;
        // Embed failure => typed error, never a silent text fallback.
        let qv = self.block_embed(query_text)?;
        let model = provider.model_id().to_string();
        let stored = self.repo.load_vectors(q.topic.as_deref(), &model)?;
        let ids: HashSet<(String, String)> = stored
            .iter()
            .map(|(t, i, _)| (t.clone(), i.clone()))
            .collect();
        let mut scored: Vec<(Experience, f32)> = Vec::new();
        for (t, i) in ids {
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
        let text = q.text.as_deref().filter(|t| !t.trim().is_empty());
        let semantic = q.semantic.as_deref().filter(|s| !s.trim().is_empty());
        match (text, semantic) {
            (None, None) => {
                let out: Vec<Experience> = self
                    .repo
                    .browse(q.topic.as_deref(), q.deep)?
                    .into_iter()
                    .filter_map(|e| self.visible(e, q.deep))
                    .collect();
                Ok(self.paginate(out, q))
            }
            (Some(t), None) => {
                let out: Vec<Experience> = self
                    .repo
                    .search_fts(t, q.topic.as_deref(), q.deep)?
                    .into_iter()
                    .map(|(e, _)| e)
                    .filter_map(|e| self.visible(e, q.deep))
                    .collect();
                Ok(self.paginate(out, q))
            }
            (None, Some(s)) => {
                let out = self.search_semantic(s, q)?;
                Ok(self.paginate(out, q))
            }
            (Some(t), Some(s)) => {
                // Combined: both rankings must succeed; embed failure is a
                // typed error, never a silent downgrade to text-only.
                let text_hits = self.repo.search_fts(t, q.topic.as_deref(), q.deep)?;
                let sem_hits = self.search_semantic(s, q)?;
                let text_keys: Vec<(String, String)> = text_hits
                    .iter()
                    .map(|(e, _)| (e.topic.clone(), e.id.clone()))
                    .collect();
                let sem_keys: Vec<(String, String)> = sem_hits
                    .iter()
                    .map(|e| (e.topic.clone(), e.id.clone()))
                    .collect();
                let fused = rrf_fuse(&text_keys, &sem_keys, RRF_K);
                let mut by_key: HashMap<(String, String), Experience> = HashMap::new();
                for (e, _) in text_hits {
                    by_key.entry((e.topic.clone(), e.id.clone())).or_insert(e);
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
}
