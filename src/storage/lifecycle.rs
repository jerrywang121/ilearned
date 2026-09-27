use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::domain::lifecycle::LifecycleConfig;
use crate::error::AppError;

use super::sqlite::to_epoch;

const DAY: i64 = 86_400;

/// Reconcile lifecycle state for all rows, then purge expired rows.
/// Runs in a single transaction: transitions (`>` comparisons on epoch
/// seconds) + `retention_started_at` for newly forgotten + purge.
pub fn reconcile_before_op(
    conn: &Connection,
    now: DateTime<Utc>,
    cfg: &LifecycleConfig,
) -> Result<(), AppError> {
    let now_s = to_epoch(&now);
    let active_s = cfg.active_period_days as i64 * DAY;
    let forget_s = cfg.forget_period_days as i64 * DAY;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let r: Result<(), AppError> = (|| {
        // Active/old -> inactive (strictly older than active period, but
        // not yet past the forget period).
        conn.execute(
            "UPDATE experiences SET state='inactive'
             WHERE state='active' AND (?1 - updated_at) > ?2 AND (?1 - updated_at) <= ?3",
            params![now_s, active_s, forget_s],
        )?;
        // Past forget period -> forgotten + retention start (if unset).
        conn.execute(
            "UPDATE experiences SET state='forgotten',
                retention_started_at = COALESCE(retention_started_at, ?1)
             WHERE state IN ('active','inactive') AND (?1 - updated_at) > ?2",
            params![now_s, forget_s],
        )?;
        purge_expired(conn, now, cfg)?;
        Ok(())
    })();
    match r {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Physically remove `deleted`/`forgotten` rows whose retention start is
/// strictly older than the retention period (plus their FTS/embed rows).
pub fn purge_expired(
    conn: &Connection,
    now: DateTime<Utc>,
    cfg: &LifecycleConfig,
) -> Result<u64, AppError> {
    let now_s = to_epoch(&now);
    let retention_s = cfg.retention_days as i64 * DAY;
    // Embeddings table may not exist in early migrations; ignore that case.
    let _ = conn.execute(
        "DELETE FROM embeddings WHERE (topic,id) IN (
            SELECT topic,id FROM experiences
            WHERE state IN ('deleted','forgotten')
              AND retention_started_at IS NOT NULL
              AND (?1 - retention_started_at) > ?2)",
        params![now_s, retention_s],
    );
    let n = conn.execute(
        "DELETE FROM experiences
         WHERE state IN ('deleted','forgotten')
           AND retention_started_at IS NOT NULL
           AND (?1 - retention_started_at) > ?2",
        params![now_s, retention_s],
    )?;
    Ok(n as u64)
}
