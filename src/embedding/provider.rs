use std::sync::Arc;

use async_trait::async_trait;

use crate::error::AppError;

/// Embedding generation backend. Optional: `None` in the service means
/// semantic search reports `EmbeddingUnavailable` instead of running.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AppError>;
    fn model_id(&self) -> &str;
    fn dimensions(&self) -> Option<usize> {
        None
    }
}

pub type DynProvider = Arc<dyn EmbeddingProvider>;
