pub mod embeddings;
pub mod encryption;
pub mod fts;
pub mod lifecycle;
pub mod repository;
pub mod sqlite;

pub use encryption::encrypt_database;
pub use repository::{ExperienceRepo, SqliteRepo};
pub use sqlite::{open_db, rebuild_fts, Db};
