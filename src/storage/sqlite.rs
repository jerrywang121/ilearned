use std::path::Path;

use rusqlite::Connection;

use crate::error::AppError;

pub use super::repository::{ExperienceRepo, SqliteRepo};

pub const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS experiences (
    topic TEXT NOT NULL,
    id TEXT NOT NULL,
    when_text TEXT NOT NULL,
    if_text TEXT NOT NULL,
    do_text TEXT NOT NULL,
    check_text TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    good_count INTEGER NOT NULL CHECK (good_count >= 0),
    bad_count INTEGER NOT NULL CHECK (bad_count >= 0),
    state TEXT NOT NULL CHECK (state IN ('active','inactive','deleted','forgotten')),
    retention_started_at INTEGER,
    PRIMARY KEY (topic, id)
);
CREATE VIRTUAL TABLE IF NOT EXISTS experiences_fts USING fts5(
    topic, when_text, if_text, do_text, check_text,
    content='experiences', content_rowid='rowid'
);
CREATE TRIGGER IF NOT EXISTS experiences_ai AFTER INSERT ON experiences BEGIN
    INSERT INTO experiences_fts(rowid, topic, when_text, if_text, do_text, check_text)
    VALUES (new.rowid, new.topic, new.when_text, new.if_text, new.do_text, new.check_text);
END;
CREATE TRIGGER IF NOT EXISTS experiences_ad AFTER DELETE ON experiences BEGIN
    INSERT INTO experiences_fts(experiences_fts, rowid, topic, when_text, if_text, do_text, check_text)
    VALUES ('delete', old.rowid, old.topic, old.when_text, old.if_text, old.do_text, old.check_text);
END;
CREATE TRIGGER IF NOT EXISTS experiences_au AFTER UPDATE ON experiences BEGIN
    INSERT INTO experiences_fts(experiences_fts, rowid, topic, when_text, if_text, do_text, check_text)
    VALUES ('delete', old.rowid, old.topic, old.when_text, old.if_text, old.do_text, old.check_text);
    INSERT INTO experiences_fts(rowid, topic, when_text, if_text, do_text, check_text)
    VALUES (new.rowid, new.topic, new.when_text, new.if_text, new.do_text, new.check_text);
END;
"#];

/// Open (creating) the SQLite DB, enable WAL, apply migrations.
pub fn open_db(path: &Path) -> Result<Connection, AppError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Internal(e.to_string()))?;
        }
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.execute_batch(MIGRATIONS[0])?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (1)",
        [],
    )?;
    // Verify the FTS5 table exists (guards against partial migrations).
    let fts: String = conn.query_row(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='experiences_fts'",
        [],
        |r| r.get(0),
    )?;
    debug_assert_eq!(fts, "experiences_fts");
    Ok(conn)
}

/// Rebuild the FTS index from the canonical table (recovery path).
pub fn rebuild_fts(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch("INSERT INTO experiences_fts(experiences_fts) VALUES('rebuild')")?;
    Ok(())
}

pub fn to_epoch(dt: &chrono::DateTime<chrono::Utc>) -> i64 {
    dt.timestamp()
}

pub fn from_epoch(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(secs, 0).unwrap_or_else(chrono::Utc::now)
}

/// Shared handle type used by tests and the service layer.
pub type Db = std::sync::Arc<std::sync::Mutex<Connection>>;

pub fn is_fts_syntax_error(e: &rusqlite::Error) -> bool {
    let m = e.to_string().to_lowercase();
    m.contains("syntax error") || m.contains("fts5") || m.contains("malformed")
}
