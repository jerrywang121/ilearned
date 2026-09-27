use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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
    /// `ILEARNED_EMBED_*` env, then config-file values. Returns `None` when
    /// disabled (no endpoint, model, or key from any source).
    pub fn from_parts(
        endpoint: Option<String>,
        model: Option<String>,
        api_key: Option<String>,
        dims: Option<usize>,
        timeout_secs: Option<u64>,
    ) -> Option<Self> {
        Self::from_parts_with_files(endpoint, model, api_key, dims, timeout_secs, None)
    }

    /// Flag/env/file resolution. Precedence per field:
    /// flags > env > file > defaults (`dims`/`timeout_secs` only).
    pub fn from_parts_with_files(
        endpoint: Option<String>,
        model: Option<String>,
        api_key: Option<String>,
        dims: Option<usize>,
        timeout_secs: Option<u64>,
        file: Option<FileEmbeddingConfig>,
    ) -> Option<Self> {
        let endpoint = endpoint.filter(|s| !s.trim().is_empty()).or_else(|| {
            std::env::var("ILEARNED_EMBED_ENDPOINT")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
        let endpoint = endpoint.or_else(|| {
            file.as_ref()
                .and_then(|f| f.endpoint.clone())
                .filter(|s| !s.trim().is_empty())
        })?;
        let model = model.filter(|s| !s.trim().is_empty()).or_else(|| {
            std::env::var("ILEARNED_EMBED_MODEL")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
        let model = model.or_else(|| {
            file.as_ref()
                .and_then(|f| f.model.clone())
                .filter(|s| !s.trim().is_empty())
        })?;
        let api_key = api_key.filter(|s| !s.trim().is_empty()).or_else(|| {
            std::env::var("ILEARNED_EMBED_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
        let api_key = api_key.or_else(|| {
            file.as_ref()
                .and_then(|f| f.api_key.clone())
                .filter(|s| !s.trim().is_empty())
        })?;
        let dims = dims
            .or_else(|| {
                std::env::var("ILEARNED_EMBED_DIMS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .or_else(|| file.as_ref().and_then(|f| f.dims));
        let timeout_secs = timeout_secs
            .or_else(|| {
                std::env::var("ILEARNED_EMBED_TIMEOUT_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .or_else(|| file.as_ref().and_then(|f| f.timeout_secs));
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

/// File-backed configuration (TOML). All fields optional; `None` means
/// "unset at this layer" so merges fall through to the next layer.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Database path (`db = "..."`).
    #[serde(default)]
    pub db: Option<PathBuf>,
    /// Bind address (`bind = "127.0.0.1:8787"`).
    #[serde(default)]
    pub bind: Option<String>,
    /// Lifecycle tuning (days).
    #[serde(default)]
    pub active_days: Option<u64>,
    #[serde(default)]
    pub forget_days: Option<u64>,
    #[serde(default)]
    pub retention_days: Option<u64>,
    /// Embedding provider settings (`[embedding]` table).
    #[serde(default)]
    pub embedding: Option<FileEmbeddingConfig>,
}

/// Embedding subset of [`FileConfig`]; mirrors `EmbeddingConfig` with options.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEmbeddingConfig {
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub dims: Option<usize>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl FileEmbeddingConfig {
    fn merge(base: Option<Self>, overlay: Option<Self>) -> Option<Self> {
        match (base, overlay) {
            (None, None) => None,
            (b, None) => b,
            (None, o) => o,
            (Some(b), Some(o)) => Some(Self {
                endpoint: o.endpoint.or(b.endpoint),
                model: o.model.or(b.model),
                api_key: o.api_key.or(b.api_key),
                dims: o.dims.or(b.dims),
                timeout_secs: o.timeout_secs.or(b.timeout_secs),
            }),
        }
    }
}

impl FileConfig {
    /// Per-field overlay: `overlay` wins wherever it sets `Some`, otherwise
    /// `base` is kept. Used for local-over-global file merging.
    pub fn merge(base: Option<Self>, overlay: Option<Self>) -> Option<Self> {
        match (base, overlay) {
            (None, None) => None,
            (b, None) => b,
            (None, o) => o,
            (Some(b), Some(o)) => Some(Self {
                db: o.db.or(b.db),
                bind: o.bind.or(b.bind),
                active_days: o.active_days.or(b.active_days),
                forget_days: o.forget_days.or(b.forget_days),
                retention_days: o.retention_days.or(b.retention_days),
                embedding: FileEmbeddingConfig::merge(b.embedding, o.embedding),
            }),
        }
    }

    /// Global file path: `$XDG_CONFIG_HOME/ilearned/config.toml`, falling
    /// back to `~/.config/ilearned/config.toml`.
    pub fn global_path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let xdg = PathBuf::from(xdg);
            if !xdg.as_os_str().is_empty() {
                return xdg.join("ilearned/config.toml");
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/ilearned/config.toml")
    }

    /// Local (project) file path: `./.ilearned/config.toml` under the CWD.
    pub fn local_path() -> PathBuf {
        PathBuf::from("./.ilearned/config.toml")
    }

    fn load_path(path: &Path) -> Result<Option<Self>, AppError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let cfg: Self = toml::from_str(&text).map_err(|e| {
                    AppError::InvalidInput(format!("invalid config file {}: {e}", path.display()))
                })?;
                Ok(Some(cfg))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AppError::Storage(format!(
                "cannot read config file {}: {e}",
                path.display()
            ))),
        }
    }

    /// Load and merge global + local files (local wins per field).
    /// Missing files are silently ignored.
    pub fn load_files() -> Result<Option<Self>, AppError> {
        let global = Self::load_path(&Self::global_path())?;
        let local = Self::load_path(&Self::local_path())?;
        Ok(Self::merge(global, local))
    }
}

/// Runtime configuration.
/// Precedence: CLI flags > `ILEARNED_*` env > local file > global file > defaults.
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
        let files = FileConfig::load_files()?;
        Self::load_with_files(
            db_path,
            bind,
            active_days,
            forget_days,
            retention_days,
            embedding,
            files,
        )
    }

    /// Flag/env/file resolution with an explicit (already merged) file layer.
    /// Precedence per field: flags > env > file > defaults.
    pub fn load_with_files(
        db_path: Option<PathBuf>,
        bind: Option<SocketAddr>,
        active_days: Option<u64>,
        forget_days: Option<u64>,
        retention_days: Option<u64>,
        embedding: Option<EmbeddingConfig>,
        file: Option<FileConfig>,
    ) -> Result<Self, AppError> {
        let file_bind: Option<SocketAddr> = match file.as_ref().and_then(|f| f.bind.clone()) {
            Some(s) => Some(s.parse().map_err(|e| {
                AppError::InvalidInput(format!("invalid bind in config file {s:?}: {e}"))
            })?),
            None => None,
        };
        let db_path = db_path
            .or_else(|| std::env::var("ILEARNED_DB").ok().map(PathBuf::from))
            .or_else(|| file.as_ref().and_then(|f| f.db.clone()))
            .unwrap_or_else(|| PathBuf::from("./ilearned.db"));
        let bind = bind
            .or_else(|| {
                std::env::var("ILEARNED_BIND")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .or(file_bind)
            .unwrap_or_else(|| "127.0.0.1:8787".parse().expect("default bind parses"));
        fn env_days(name: &str) -> Option<u64> {
            std::env::var(name).ok().and_then(|s| s.parse::<u64>().ok())
        }
        let file_days = |pick: fn(&FileConfig) -> Option<u64>| file.as_ref().and_then(pick);
        let lifecycle = LifecycleConfig {
            active_period_days: active_days
                .or_else(|| env_days("ILEARNED_ACTIVE_DAYS"))
                .or_else(|| file_days(|f| f.active_days))
                .unwrap_or(LifecycleConfig::default().active_period_days),
            forget_period_days: forget_days
                .or_else(|| env_days("ILEARNED_FORGET_DAYS"))
                .or_else(|| file_days(|f| f.forget_days))
                .unwrap_or(LifecycleConfig::default().forget_period_days),
            retention_days: retention_days
                .or_else(|| env_days("ILEARNED_RETENTION_DAYS"))
                .or_else(|| file_days(|f| f.retention_days))
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
