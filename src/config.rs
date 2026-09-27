use std::net::SocketAddr;
use std::path::PathBuf;

use crate::domain::lifecycle::LifecycleConfig;
use crate::error::AppError;

/// Optional embedding provider settings (OpenAI-compatible API).
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub dims: usize,
    pub timeout_secs: u64,
}

/// Runtime configuration. Precedence: CLI flags > `ILEARNED_*` env > defaults.
#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub bind: SocketAddr,
    pub lifecycle: LifecycleConfig,
    pub embedding: Option<EmbeddingConfig>,
}

impl Config {
    pub fn load(
        db_path: Option<PathBuf>,
        bind: Option<SocketAddr>,
        embedding: Option<EmbeddingConfig>,
    ) -> Result<Self, AppError> {
        let db_path = db_path
            .or_else(|| std::env::var("ILEARNED_DB").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("./ilearned.db"));
        let bind = bind
            .or_else(|| {
                std::env::var("ILEARNED_BIND")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or_else(|| "127.0.0.1:8787".parse().expect("default bind parses"));
        Ok(Self {
            db_path,
            bind,
            lifecycle: LifecycleConfig::default(),
            embedding,
        })
    }
}
