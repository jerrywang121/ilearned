use async_trait::async_trait;

use crate::embedding::provider::EmbeddingProvider;
use crate::error::AppError;

/// Deterministic hash-bucket embedding for tests: lowercase alphanumeric
/// tokens hashed into 64 buckets, L2-normalized.
#[derive(Clone)]
pub struct FakeEmbeddingProvider {
    dim: usize,
    model: String,
}

impl FakeEmbeddingProvider {
    pub fn new() -> Self {
        Self {
            dim: 64,
            model: "fake-test".to_string(),
        }
    }
}

impl Default for FakeEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn hash_token(tok: &str) -> u64 {
    // FNV-1a 64-bit.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in tok.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn fake_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut v = vec![0f32; dim];
    for tok in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        v[(hash_token(tok) as usize) % dim] += 1.0;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

/// Provider that always fails — exercises the typed-error paths.
#[derive(Clone, Copy)]
pub struct FailingEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for FakeEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AppError> {
        Ok(fake_embed(text, self.dim))
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> Option<usize> {
        Some(self.dim)
    }
}

#[async_trait]
impl EmbeddingProvider for FailingEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, AppError> {
        Err(AppError::EmbeddingUnavailable(
            "fake provider fails".to_string(),
        ))
    }

    fn model_id(&self) -> &str {
        "fake-failing"
    }
}
