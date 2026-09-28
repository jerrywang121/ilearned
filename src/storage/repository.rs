use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Row};

use crate::domain::commands::{ClearCommand, SearchQuery};
use crate::domain::experience::{Experience, State};
use crate::domain::lifecycle::{is_eligible, LifecycleConfig};
use crate::error::AppError;

use super::lifecycle::reconcile_before_op;
use super::sqlite::{from_epoch, is_fts_syntax_error, open_db, to_epoch};

fn parse_state(s: &str) -> Result<State, AppError> {
    match s {
        "active" => Ok(State::Active),
        "inactive" => Ok(State::Inactive),
        "deleted" => Ok(State::Deleted),
        "forgotten" => Ok(State::Forgotten),
        other => Err(AppError::Internal(format!("unknown state {other}"))),
    }
}

fn state_str(s: &State) -> &'static str {
    match s {
        State::Active => "active",
        State::Inactive => "inactive",
        State::Deleted => "deleted",
        State::Forgotten => "forgotten",
    }
}

fn row_to_exp(row: &Row) -> rusqlite::Result<Experience> {
    let state_s: String = row.get("state")?;
    // Unknown states are data corruption: surface as a row error (via the
    // shared parser) rather than silently mapping to Active.
    let state = parse_state(&state_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            9,
            rusqlite::types::Type::Text,
            e.to_string().into(),
        )
    })?;
    Ok(Experience {
        topic: row.get("topic")?,
        id: row.get("id")?,
        when_text: row.get("when_text")?,
        if_text: row.get("if_text")?,
        do_text: row.get("do_text")?,
        check_text: row.get("check_text")?,
        updated_at: super::sqlite::from_epoch(row.get::<_, i64>("updated_at")?),
        good_count: row.get::<_, i64>("good_count")? as u64,
        bad_count: row.get::<_, i64>("bad_count")? as u64,
        state,
    })
}

pub trait ExperienceRepo: Send + Sync {
    fn insert(&self, e: &Experience) -> Result<(), AppError>;
    fn get(&self, topic: &str, id: &str) -> Result<Option<Experience>, AppError>;
    fn update(&self, e: &Experience) -> Result<(), AppError>;
    /// Soft-delete: mark deleted + refresh updated_at + set retention start.
    /// Returns true if a row was touched (idempotent on already-deleted).
    fn soft_delete(&self, topic: &str, id: &str, now: DateTime<Utc>) -> Result<bool, AppError>;
    fn clear(&self, cmd: &ClearCommand, now: DateTime<Utc>) -> Result<u64, AppError>;
    /// FTS5/BM25 text search over eligible rows. Returns (experience, bm25 rank).
    fn search_fts(
        &self,
        text: &str,
        topic: Option<&str>,
        deep: bool,
    ) -> Result<Vec<(Experience, f32)>, AppError>;
    /// Distinct topics honoring visibility: only topics with >=1
    /// active record (deep=false), or >=1 active/inactive record
    /// (deep=true). Sorted ascending. `deleted`/`forgotten` never contribute.
    fn distinct_topics(&self, deep: bool) -> Result<Vec<String>, AppError>;
    /// Paginated browse ordered by updated_at DESC (service applies limit/offset).
    fn browse(&self, topic: Option<&str>, deep: bool) -> Result<Vec<Experience>, AppError>;
    /// Run lifecycle reconcile + purge (implemented via storage::lifecycle).
    fn reconcile(&self, now: DateTime<Utc>, cfg: &LifecycleConfig) -> Result<(), AppError> {
        reconcile_before_op(&self.conn_ref(), now, cfg)
    }
    /// Borrow the underlying connection for lifecycle transactions.
    /// Default panics; SqliteRepo overrides.
    fn conn_ref(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        panic!("conn_ref not implemented")
    }
}

#[derive(Clone)]
pub struct SqliteRepo {
    db: super::sqlite::Db,
}

impl SqliteRepo {
    pub fn open(path: &std::path::Path) -> Result<Self, AppError> {
        let conn = open_db(path)?;
        Ok(Self {
            db: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    /// Test/service escape hatch for lifecycle transactions.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.db.lock().expect("db lock")
    }
}

impl ExperienceRepo for SqliteRepo {
    fn conn_ref(&self) -> std::sync::MutexGuard<'_, rusqlite::Connection> {
        self.db.lock().expect("db lock")
    }

    fn insert(&self, e: &Experience) -> Result<(), AppError> {
        let db = self.db.lock().expect("db lock");
        db.execute(
            "INSERT INTO experiences (topic,id,when_text,if_text,do_text,check_text,updated_at,good_count,bad_count,state,retention_started_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL)",
            params![
                e.topic,
                e.id,
                e.when_text,
                e.if_text,
                e.do_text,
                e.check_text,
                to_epoch(&e.updated_at),
                e.good_count as i64,
                e.bad_count as i64,
                state_str(&e.state),
            ],
        )?;
        Ok(())
    }

    fn get(&self, topic: &str, id: &str) -> Result<Option<Experience>, AppError> {
        let db = self.db.lock().expect("db lock");
        let row = db
            .query_row(
                "SELECT topic,id,when_text,if_text,do_text,check_text,updated_at,good_count,bad_count,state
                 FROM experiences WHERE topic=?1 AND id=?2",
                params![topic, id],
                row_to_exp,
            )
            .optional()?;
        Ok(row)
    }

    fn update(&self, e: &Experience) -> Result<(), AppError> {
        let db = self.db.lock().expect("db lock");
        // Clearing retention metadata is part of every canonical update
        // (modify/promote/downgrade restore the record to active life).
        db.execute(
            "UPDATE experiences SET when_text=?3,if_text=?4,do_text=?5,check_text=?6,updated_at=?7,
             good_count=?8,bad_count=?9,state=?10,retention_started_at=NULL
             WHERE topic=?1 AND id=?2",
            params![
                e.topic,
                e.id,
                e.when_text,
                e.if_text,
                e.do_text,
                e.check_text,
                to_epoch(&e.updated_at),
                e.good_count as i64,
                e.bad_count as i64,
                state_str(&e.state),
            ],
        )?;
        Ok(())
    }

    fn soft_delete(&self, topic: &str, id: &str, now: DateTime<Utc>) -> Result<bool, AppError> {
        let db = self.db.lock().expect("db lock");
        let n = db.execute(
            "UPDATE experiences SET state='deleted', updated_at=?3, retention_started_at=?3
             WHERE topic=?1 AND id=?2",
            params![topic, id, to_epoch(&now)],
        )?;
        Ok(n > 0)
    }

    fn clear(&self, cmd: &ClearCommand, now: DateTime<Utc>) -> Result<u64, AppError> {
        let db = self.db.lock().expect("db lock");
        let n = match cmd {
            ClearCommand::Topic(t) => db.execute(
                "UPDATE experiences SET state='deleted', updated_at=?2, retention_started_at=?2
                 WHERE topic=?1 AND state != 'deleted'",
                params![t, to_epoch(&now)],
            )?,
            ClearCommand::All => db.execute(
                "UPDATE experiences SET state='deleted', updated_at=?1, retention_started_at=?1
                 WHERE state != 'deleted'",
                params![to_epoch(&now)],
            )?,
        };
        Ok(n as u64)
    }

    fn search_fts(
        &self,
        text: &str,
        topic: Option<&str>,
        deep: bool,
    ) -> Result<Vec<(Experience, f32)>, AppError> {
        let db = self.db.lock().expect("db lock");
        // Topic filter stays outside the FTS MATCH expression.
        let sql = if topic.is_some() {
            "SELECT e.topic,e.id,e.when_text,e.if_text,e.do_text,e.check_text,
                    e.updated_at,e.good_count,e.bad_count,e.state,
                    bm25(experiences_fts) AS rank
             FROM experiences_fts JOIN experiences e ON e.rowid = experiences_fts.rowid
             WHERE experiences_fts MATCH ?1 AND e.topic = ?2
             ORDER BY rank"
        } else {
            "SELECT e.topic,e.id,e.when_text,e.if_text,e.do_text,e.check_text,
                    e.updated_at,e.good_count,e.bad_count,e.state,
                    bm25(experiences_fts) AS rank
             FROM experiences_fts JOIN experiences e ON e.rowid = experiences_fts.rowid
             WHERE experiences_fts MATCH ?1
             ORDER BY rank"
        };
        let mut stmt = db.prepare(sql).map_err(|e| {
            if is_fts_syntax_error(&e) {
                AppError::InvalidFtsSyntax(e.to_string())
            } else {
                AppError::from(e)
            }
        })?;
        let rows: Result<Vec<(Experience, f32)>, rusqlite::Error> = if let Some(t) = topic {
            stmt.query_map(params![text, t], |row| {
                let rank: f64 = row.get("rank")?;
                Ok((row_to_exp(row)?, rank as f32))
            })?
            .collect()
        } else {
            stmt.query_map(params![text], |row| {
                let rank: f64 = row.get("rank")?;
                Ok((row_to_exp(row)?, rank as f32))
            })?
            .collect()
        };
        let rows = rows.map_err(|e| {
            if is_fts_syntax_error(&e) {
                AppError::InvalidFtsSyntax(e.to_string())
            } else {
                AppError::from(e)
            }
        })?;
        // Lifecycle eligibility always applies after the text match.
        Ok(rows
            .into_iter()
            .filter(|(e, _)| is_eligible(&e.state, deep))
            .collect())
    }

    fn browse(&self, topic: Option<&str>, deep: bool) -> Result<Vec<Experience>, AppError> {
        let db = self.db.lock().expect("db lock");
        let sql = if topic.is_some() {
            "SELECT topic,id,when_text,if_text,do_text,check_text,updated_at,good_count,bad_count,state
             FROM experiences WHERE topic=?1 ORDER BY updated_at DESC"
        } else {
            "SELECT topic,id,when_text,if_text,do_text,check_text,updated_at,good_count,bad_count,state
             FROM experiences ORDER BY updated_at DESC"
        };
        let mut stmt = db.prepare(sql)?;
        let rows: Vec<Experience> = if let Some(t) = topic {
            stmt.query_map(params![t], row_to_exp)?
                .collect::<Result<_, _>>()?
        } else {
            stmt.query_map([], row_to_exp)?.collect::<Result<_, _>>()?
        };
        Ok(rows
            .into_iter()
            .filter(|e| is_eligible(&e.state, deep))
            .collect())
    }

    fn distinct_topics(&self, deep: bool) -> Result<Vec<String>, AppError> {
        let db = self.db.lock().expect("db lock");
        let sql = if deep {
            "SELECT DISTINCT topic FROM experiences
             WHERE state IN ('active','inactive') ORDER BY topic ASC"
        } else {
            "SELECT DISTINCT topic FROM experiences
             WHERE state = 'active' ORDER BY topic ASC"
        };
        let mut stmt = db.prepare(sql)?;
        let out: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(out)
    }
}

/// Build a `SearchQuery`-shaped browse for callers that already hold one
/// (keeps service code uniform). Pagination is applied by the service.
#[allow(dead_code)]
pub fn browse_for_query(
    repo: &impl ExperienceRepo,
    q: &SearchQuery,
) -> Result<Vec<Experience>, AppError> {
    repo.browse(q.topic.as_deref(), q.deep)
}

pub fn _epoch_roundtrip(dt: &DateTime<Utc>) -> DateTime<Utc> {
    from_epoch(to_epoch(dt))
}

#[allow(dead_code)]
pub fn _parse_state(s: &str) -> Result<State, AppError> {
    parse_state(s)
}

pub fn _now_utc() -> DateTime<Utc> {
    Utc::now()
}
