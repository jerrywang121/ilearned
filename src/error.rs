use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("experience not found: ({topic}, {id})")]
    NotFound { topic: String, id: String },
    #[error("embedding provider unavailable: {0}")]
    EmbeddingUnavailable(String),
    #[error("invalid FTS syntax: {0}")]
    InvalidFtsSyntax(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Storage(e.to_string())
    }
}
