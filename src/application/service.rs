use chrono::Utc;
use uuid::Uuid;

use crate::domain::commands::{
    AddCommand, ClearCommand, FeedbackCommand, ModifyCommand, SearchQuery,
};
use crate::domain::experience::{Experience, State};
use crate::domain::lifecycle::LifecycleConfig;
use crate::error::AppError;
use crate::storage::repository::ExperienceRepo;

pub const MAX_LIMIT: u32 = 100;

/// The sole entry point for adapters. Owns lifecycle, ranking, transactions.
pub struct MemoryService<R> {
    repo: R,
    lifecycle: LifecycleConfig,
}

impl<R: ExperienceRepo> MemoryService<R> {
    pub fn new(repo: R, lifecycle: LifecycleConfig) -> Self {
        Self { repo, lifecycle }
    }

    /// Test escape hatch: direct repo access.
    pub fn repo(&self) -> &R {
        &self.repo
    }

    fn reconcile(&self) -> Result<(), AppError> {
        // SqliteRepo-specific: downcast via the conn() escape hatch is not
        // object-safe, so lifecycle runs through a repository-provided hook.
        self.repo.reconcile(Utc::now(), &self.lifecycle)
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
                Ok(()) => return Ok(e),
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

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<Experience>, AppError> {
        self.reconcile()?;
        if q.semantic.is_some() {
            // Semantic path lands in Task 4; typed error, never silent fallback.
            return Err(AppError::EmbeddingUnavailable(
                "semantic search requires an embedding provider (see Task 4)".to_string(),
            ));
        }
        let mut out: Vec<Experience> = if let Some(text) = q.text.as_deref() {
            self.repo
                .search_fts(text, q.topic.as_deref(), q.deep)?
                .into_iter()
                .map(|(e, _)| e)
                .filter_map(|e| self.visible(e, q.deep))
                .collect()
        } else {
            self.repo
                .browse(q.topic.as_deref(), q.deep)?
                .into_iter()
                .filter_map(|e| self.visible(e, q.deep))
                .collect()
        };
        let limit = q.limit.min(MAX_LIMIT) as usize;
        let offset = q.offset as usize;
        if offset >= out.len() || limit == 0 {
            return Ok(vec![]);
        }
        out.truncate(offset + limit);
        Ok(out[offset..].to_vec())
    }
}
