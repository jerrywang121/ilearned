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
    PRIMARY KEY (topic, id, model)
);
"#;

pub trait VectorStore: Send + Sync {
    fn upsert_vector(&self, topic: &str, id: &str, model: &str, v: &[f32]) -> Result<(), AppError>;
    fn load_vectors(
        &self,
        topic: Option<&str>,
        model: &str,
    ) -> Result<Vec<(String, String, Vec<f32>)>, AppError>;
    fn delete_vectors(&self, topic: &str, id: &str) -> Result<(), AppError>;
}

fn encode(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn decode(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

impl VectorStore for SqliteRepo {
    fn upsert_vector(&self, topic: &str, id: &str, model: &str, v: &[f32]) -> Result<(), AppError> {
        ensure_table(self)?;
        let db = self.conn();
        db.execute(
            "INSERT INTO embeddings (topic,id,model,dims,vec) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(topic,id,model) DO UPDATE SET dims=excluded.dims, vec=excluded.vec",
            params![topic, id, model, v.len() as i64, encode(v)],
        )?;
        Ok(())
    }

    fn load_vectors(
        &self,
        topic: Option<&str>,
        model: &str,
    ) -> Result<Vec<(String, String, Vec<f32>)>, AppError> {
        ensure_table(self)?;
        let db = self.conn();
        let sql = if topic.is_some() {
            "SELECT topic,id,vec FROM embeddings WHERE topic=?1 AND model=?2"
        } else {
            "SELECT topic,id,vec FROM embeddings WHERE model=?1"
        };
        let mut stmt = db.prepare(sql)?;
        let rows: Vec<(String, String, Vec<f32>)> = if let Some(t) = topic {
            stmt.query_map(params![t, model], |row| {
                let bytes: Vec<u8> = row.get(2)?;
                Ok((row.get(0)?, row.get(1)?, decode(&bytes)))
            })?
            .collect::<Result<_, _>>()?
        } else {
            stmt.query_map(params![model], |row| {
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
}

fn ensure_table(repo: &SqliteRepo) -> Result<(), AppError> {
    repo.conn().execute_batch(EMBEDDINGS_DDL)?;
    Ok(())
}
