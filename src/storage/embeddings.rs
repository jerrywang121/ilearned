use rusqlite::params;

use crate::error::AppError;
use crate::storage::repository::SqliteRepo;

pub const EMBEDDINGS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS embeddings (
    topic TEXT NOT NULL,
    id TEXT NOT NULL,
    model TEXT NOT NULL,
    dims INTEGER NOT NULL,
    vec BLOB NOT NULL,
    PRIMARY KEY (topic, id, model, dims)
);
"#;

pub trait VectorStore: Send + Sync {
    fn upsert_vector(&self, topic: &str, id: &str, model: &str, v: &[f32]) -> Result<(), AppError>;
    fn load_vectors(
        &self,
        topic: Option<&str>,
        model: &str,
        dims: usize,
    ) -> Result<Vec<(String, String, Vec<f32>)>, AppError>;
    fn delete_vectors(&self, topic: &str, id: &str) -> Result<(), AppError>;
    fn prune_vectors(&self, model: &str, dims: usize) -> Result<u64, AppError>;
}

fn encode(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn decode(bytes: &[u8]) -> Vec<f32> {
    let (chunks, _rem) = bytes.as_chunks::<4>();
    chunks.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

impl VectorStore for SqliteRepo {
    fn upsert_vector(&self, topic: &str, id: &str, model: &str, v: &[f32]) -> Result<(), AppError> {
        ensure_table(self)?;
        let db = self.conn();
        db.execute(
            "INSERT INTO embeddings (topic,id,model,dims,vec) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(topic,id,model,dims) DO UPDATE SET vec=excluded.vec",
            params![topic, id, model, v.len() as i64, encode(v)],
        )?;
        Ok(())
    }

    fn load_vectors(
        &self,
        topic: Option<&str>,
        model: &str,
        dims: usize,
    ) -> Result<Vec<(String, String, Vec<f32>)>, AppError> {
        ensure_table(self)?;
        let db = self.conn();
        let sql = if topic.is_some() {
            "SELECT topic,id,vec FROM embeddings WHERE topic=?1 AND model=?2 AND dims=?3"
        } else {
            "SELECT topic,id,vec FROM embeddings WHERE model=?1 AND dims=?2"
        };
        let mut stmt = db.prepare(sql)?;
        let rows: Vec<(String, String, Vec<f32>)> = if let Some(t) = topic {
            stmt.query_map(params![t, model, dims as i64], |row| {
                let bytes: Vec<u8> = row.get(2)?;
                Ok((row.get(0)?, row.get(1)?, decode(&bytes)))
            })?
            .collect::<Result<_, _>>()?
        } else {
            stmt.query_map(params![model, dims as i64], |row| {
                let bytes: Vec<u8> = row.get(2)?;
                Ok((row.get(0)?, row.get(1)?, decode(&bytes)))
            })?
            .collect::<Result<_, _>>()?
        };
        Ok(rows)
    }

    fn delete_vectors(&self, topic: &str, id: &str) -> Result<(), AppError> {
        ensure_table(self)?;
        let db = self.conn();
        db.execute(
            "DELETE FROM embeddings WHERE topic=?1 AND id=?2",
            params![topic, id],
        )?;
        Ok(())
    }

    fn prune_vectors(&self, model: &str, dims: usize) -> Result<u64, AppError> {
        ensure_table(self)?;
        let db = self.conn();
        let n = db.execute(
            "DELETE FROM embeddings WHERE model != ?1 OR dims != ?2",
            params![model, dims as i64],
        )?;
        Ok(n as u64)
    }
}

fn ensure_table(repo: &SqliteRepo) -> Result<(), AppError> {
    let mut db = repo.conn();
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='embeddings')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        db.execute_batch(EMBEDDINGS_DDL)?;
        return Ok(());
    }

    let mut stmt = db.prepare("PRAGMA table_info(embeddings)")?;
    let mut primary_key: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(5)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    primary_key.sort_by_key(|(position, _)| *position);
    let uses_composite_key = primary_key
        == vec![
            (1, "topic".to_string()),
            (2, "id".to_string()),
            (3, "model".to_string()),
            (4, "dims".to_string()),
        ];
    if uses_composite_key {
        return Ok(());
    }

    let tx = db.transaction()?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS embeddings_new;
         CREATE TABLE embeddings_new (
             topic TEXT NOT NULL,
             id TEXT NOT NULL,
             model TEXT NOT NULL,
             dims INTEGER NOT NULL,
             vec BLOB NOT NULL,
             PRIMARY KEY (topic, id, model, dims)
         );
         INSERT INTO embeddings_new (topic,id,model,dims,vec)
             SELECT topic,id,model,dims,vec FROM embeddings;
         DROP TABLE embeddings;
         ALTER TABLE embeddings_new RENAME TO embeddings;",
    )?;
    tx.commit()?;
    Ok(())
}
