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

impl EmbeddingConfig {
    pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
    pub const DEFAULT_DIMS: usize = 1536;

    /// Build from explicit parts (CLI flags); missing pieces fall back to
    /// `ILEARNED_EMBED_*` env. Returns `None` when disabled (no endpoint,
    /// model, or key from either source).
    pub fn from_parts(
        endpoint: Option<String>,
        model: Option<String>,
        api_key: Option<String>,
        dims: Option<usize>,
        timeout_secs: Option<u64>,
    ) -> Option<Self> {
        let endpoint = endpoint.filter(|s| !s.trim().is_empty()).or_else(|| {
            std::env::var("ILEARNED_EMBED_ENDPOINT")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })?;
        let model = model.filter(|s| !s.trim().is_empty()).or_else(|| {
            std::env::var("ILEARNED_EMBED_MODEL")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })?;
        let api_key = api_key.filter(|s| !s.trim().is_empty()).or_else(|| {
            std::env::var("ILEARNED_EMBED_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })?;
        let dims = dims.or_else(|| {
            std::env::var("ILEARNED_EMBED_DIMS")
                .ok()
                .and_then(|s| s.parse().ok())
        });
        let timeout_secs = timeout_secs.or_else(|| {
            std::env::var("ILEARNED_EMBED_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
        });
        Some(Self {
            endpoint,
            model,
            api_key,
            dims: dims.unwrap_or(Self::DEFAULT_DIMS),
            timeout_secs: timeout_secs.unwrap_or(Self::DEFAULT_TIMEOUT_SECS),
        })
    }

    /// Env-only construction (used as fallback when no flags given).
    pub fn from_env() -> Option<Self> {
        Self::from_parts(None, None, None, None, None)
    }
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
        active_days: Option<u64>,
        forget_days: Option<u64>,
        retention_days: Option<u64>,
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
        fn env_days(name: &str) -> Option<u64> {
            std::env::var(name).ok().and_then(|s| s.parse::<u64>().ok())
        }
        let lifecycle = LifecycleConfig {
            active_period_days: active_days
                .or_else(|| env_days("ILEARNED_ACTIVE_DAYS"))
                .unwrap_or(LifecycleConfig::default().active_period_days),
            forget_period_days: forget_days
                .or_else(|| env_days("ILEARNED_FORGET_DAYS"))
                .unwrap_or(LifecycleConfig::default().forget_period_days),
            retention_days: retention_days
                .or_else(|| env_days("ILEARNED_RETENTION_DAYS"))
                .unwrap_or(LifecycleConfig::default().retention_days),
        };
        Ok(Self {
            db_path,
            bind,
            lifecycle,
            embedding,
        })
    }
}
