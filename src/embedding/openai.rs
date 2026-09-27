use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::EmbeddingConfig;
use crate::embedding::provider::EmbeddingProvider;
use crate::error::AppError;

pub struct OpenAiEmbeddingProvider {
    endpoint: String,
    model: String,
    api_key: String,
    timeout: Duration,
    client: reqwest::Client,
}

impl OpenAiEmbeddingProvider {
    pub fn new(cfg: &EmbeddingConfig) -> Self {
        Self {
            endpoint: cfg.endpoint.clone(),
            model: cfg.model.clone(),
            api_key: cfg.api_key.clone(),
            timeout: Duration::from_secs(cfg.timeout_secs.max(1)),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, AppError> {
        let url = format!("{}/embeddings", self.endpoint.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .timeout(self.timeout)
            .json(&EmbedRequest {
                model: &self.model,
                input: text,
            })
            .send()
            .await
            .map_err(|e| AppError::EmbeddingUnavailable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(AppError::EmbeddingUnavailable(format!(
                "provider status {}",
                resp.status()
            )));
        }
        let body: EmbedResponse = resp
            .json()
            .await
            .map_err(|e| AppError::EmbeddingUnavailable(e.to_string()))?;
        body.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| AppError::EmbeddingUnavailable("empty embedding".to_string()))
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
