use std::fmt::Display;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

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

    /// Build from explicit values; missing pieces fall back to
    /// `ILEARNED_EMBED_*` env, then config-file values. Returns `Ok(None)` when
    /// disabled (no endpoint, model, or key from any source).
    pub fn from_parts(
        endpoint: Option<String>,
        model: Option<String>,
        api_key: Option<String>,
        dims: Option<usize>,
        timeout_secs: Option<u64>,
    ) -> Result<Option<Self>, AppError> {
        Self::from_parts_with_files(endpoint, model, api_key, dims, timeout_secs, None)
    }

    /// Explicit/env/file resolution. Precedence per field:
    /// explicit values > env > file > defaults (`dims`/`timeout_secs` only).
    pub fn from_parts_with_files(
        endpoint: Option<String>,
        model: Option<String>,
        api_key: Option<String>,
        dims: Option<usize>,
        timeout_secs: Option<u64>,
        file: Option<FileEmbeddingConfig>,
    ) -> Result<Option<Self>, AppError> {
        let dims = resolve_setting(
            dims,
            "ILEARNED_EMBED_DIMS",
            file.as_ref().and_then(|f| f.dims),
        )?;
        let timeout_secs = resolve_setting(
            timeout_secs,
            "ILEARNED_EMBED_TIMEOUT_SECS",
            file.as_ref().and_then(|f| f.timeout_secs),
        )?;
        let endpoint = resolve_string(
            endpoint,
            "ILEARNED_EMBED_ENDPOINT",
            file.as_ref().and_then(|f| f.endpoint.clone()),
        )?;
        let Some(endpoint) = endpoint else {
            return Ok(None);
        };
        let model = resolve_string(
            model,
            "ILEARNED_EMBED_MODEL",
            file.as_ref().and_then(|f| f.model.clone()),
        )?;
        let Some(model) = model else {
            return Ok(None);
        };
        let api_key = resolve_string(
            api_key,
            "ILEARNED_EMBED_API_KEY",
            file.as_ref().and_then(|f| f.api_key.clone()),
        )?;
        let Some(api_key) = api_key else {
            return Ok(None);
        };
        Ok(Some(Self {
            endpoint,
            model,
            api_key,
            dims: dims.unwrap_or(Self::DEFAULT_DIMS),
            timeout_secs: timeout_secs.unwrap_or(Self::DEFAULT_TIMEOUT_SECS),
        }))
    }

    /// Env-only construction.
    pub fn from_env() -> Result<Option<Self>, AppError> {
        Self::from_parts(None, None, None, None, None)
    }
}

fn env_string(name: &str) -> Result<Option<String>, AppError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(AppError::InvalidInput(format!(
            "environment variable {name} is not valid UTF-8"
        ))),
    }
}

fn parse_env<T>(name: &str) -> Result<Option<T>, AppError>
where
    T: FromStr,
    T::Err: Display,
{
    let Some(value) = env_string(name)? else {
        return Ok(None);
    };
    value.parse().map(Some).map_err(|e| {
        AppError::InvalidInput(format!(
            "invalid value for environment variable {name}: {e}"
        ))
    })
}

fn resolve_setting<T>(
    explicit: Option<T>,
    env_name: &str,
    file: Option<T>,
) -> Result<Option<T>, AppError>
where
    T: FromStr,
    T::Err: Display,
{
    if explicit.is_some() {
        return Ok(explicit);
    }
    Ok(parse_env(env_name)?.or(file))
}

fn resolve_string(
    explicit: Option<String>,
    env_name: &str,
    file: Option<String>,
) -> Result<Option<String>, AppError> {
    if let Some(value) = explicit.filter(|s| !s.trim().is_empty()) {
        return Ok(Some(value));
    }
    if let Some(value) = env_string(env_name)?.filter(|s| !s.trim().is_empty()) {
        return Ok(Some(value));
    }
    Ok(file.filter(|s| !s.trim().is_empty()))
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
    /// Demote auto-delete threshold (0.0 disables, 1.0 deletes on first
    /// demote). Must be within `0.0..=1.0`.
    #[serde(default)]
    pub auto_delete_threshold: Option<f64>,
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
                auto_delete_threshold: o.auto_delete_threshold.or(b.auto_delete_threshold),
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

    /// Local (project) database fallback: `./.ilearned/ilearned.db` under
    /// the CWD. Used only when no database is configured anywhere.
    pub fn local_db_fallback() -> PathBuf {
        PathBuf::from("./.ilearned/ilearned.db")
    }

    /// Global database fallback: `$XDG_DATA_HOME/ilearned/ilearned.db`,
    /// falling back to `~/.local/share/ilearned/ilearned.db`.
    /// Returns `None` when neither environment variable provides a usable
    /// base directory.
    pub fn global_data_db_path() -> Option<PathBuf> {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|path| !path.is_empty()) {
            return Some(PathBuf::from(xdg).join("ilearned/ilearned.db"));
        }
        std::env::var_os("HOME")
            .filter(|path| !path.is_empty())
            .map(|home| PathBuf::from(home).join(".local/share/ilearned/ilearned.db"))
    }

    /// Generate the default configuration directly from the executable.
    ///
    /// The database and provider credentials remain commented because their
    /// values are installation-specific; the remaining settings document the
    /// runtime defaults in a ready-to-edit TOML file.
    pub fn default_config_toml() -> String {
        format!(
            "# ilearned configuration\n\n# Database path (optional).\n# db = \"./.ilearned/ilearned.db\"\n\nbind = \"127.0.0.1:8787\"\nactive_days = {}\nforget_days = {}\nretention_days = {}\nauto_delete_threshold = {}\n\n[embedding]\n# endpoint = \"http://localhost:11434/v1\"\n# model = \"nomic-embed-text\"\n# api_key = \"your-api-key\"\ndims = {}\ntimeout_secs = {}\n",
            LifecycleConfig::default().active_period_days,
            LifecycleConfig::default().forget_period_days,
            LifecycleConfig::default().retention_days,
            LifecycleConfig::default().auto_delete_threshold,
            EmbeddingConfig::DEFAULT_DIMS,
            EmbeddingConfig::DEFAULT_TIMEOUT_SECS,
        )
    }

    /// Create a generated configuration file without overwriting an existing
    /// file. The target's parent directory is created when necessary.
    pub fn init(path: &Path) -> Result<(), AppError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Storage(format!(
                    "cannot create config directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = match options.open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(AppError::InvalidInput(format!(
                    "config file already exists: {}",
                    path.display()
                )))
            }
            Err(e) => {
                return Err(AppError::Storage(format!(
                    "cannot create config file {}: {e}",
                    path.display()
                )))
            }
        };
        file.write_all(Self::default_config_toml().as_bytes())
            .map_err(|e| {
                AppError::Storage(format!("cannot write config file {}: {e}", path.display()))
            })
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
    /// Missing default files are silently ignored.
    pub fn load_files() -> Result<Option<Self>, AppError> {
        Self::load_files_with_override(None)
    }

    /// Load global and local files, then overlay an explicitly selected CLI
    /// file. The explicit file must exist and wins per field over both defaults.
    pub fn load_files_with_override(
        override_path: Option<&Path>,
    ) -> Result<Option<Self>, AppError> {
        Self::load_files_with_sources(override_path).map(|(config, _)| config)
    }

    /// Load the default layers and return the paths that actually existed and
    /// participated in resolution, in precedence order.
    pub fn load_files_with_sources(
        override_path: Option<&Path>,
    ) -> Result<(Option<Self>, Vec<PathBuf>), AppError> {
        let mut paths = Vec::new();
        let global_path = Self::global_path();
        let global = Self::load_path(&global_path)?;
        if global.is_some() {
            paths.push(global_path);
        }
        let local_path = Self::local_path();
        let local = Self::load_path(&local_path)?;
        if local.is_some() {
            paths.push(local_path);
        }
        let base = Self::merge(global, local);
        let override_config = match override_path {
            Some(path) => match Self::load_path(path)? {
                Some(config) => {
                    paths.push(path.to_path_buf());
                    Some(config)
                }
                None => {
                    return Err(AppError::InvalidInput(format!(
                        "config file {} does not exist",
                        path.display()
                    )))
                }
            },
            None => None,
        };
        Ok((Self::merge(base, override_config), paths))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedConfig {
    pub db: Option<String>,
    pub bind: String,
    pub active_days: u64,
    pub forget_days: u64,
    pub retention_days: u64,
    pub auto_delete_threshold: f64,
    pub embedding: Option<ResolvedEmbeddingConfig>,
}

impl ResolvedConfig {
    pub fn from_file(file: Option<FileConfig>) -> Result<Self, AppError> {
        let db = resolve_database_path(None, file.as_ref())
            .map(|path| path.to_string_lossy().into_owned());
        let bind = resolve_bind(None, file.as_ref())?;
        let lifecycle = resolve_lifecycle(None, None, None, file.as_ref())?;
        let embedding = resolve_embedding_for_show(file.as_ref())?;
        Ok(Self {
            db,
            bind: bind.to_string(),
            active_days: lifecycle.active_period_days,
            forget_days: lifecycle.forget_period_days,
            retention_days: lifecycle.retention_days,
            auto_delete_threshold: lifecycle.auto_delete_threshold,
            embedding,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedEmbeddingConfig {
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub api_key_configured: bool,
    pub dims: usize,
    pub timeout_secs: u64,
}

fn resolve_embedding_for_show(
    file: Option<&FileConfig>,
) -> Result<Option<ResolvedEmbeddingConfig>, AppError> {
    let file = file.and_then(|config| config.embedding.as_ref());
    let dims = resolve_setting(None, "ILEARNED_EMBED_DIMS", file.and_then(|f| f.dims))?
        .unwrap_or(EmbeddingConfig::DEFAULT_DIMS);
    let timeout_secs = resolve_setting(
        None,
        "ILEARNED_EMBED_TIMEOUT_SECS",
        file.and_then(|f| f.timeout_secs),
    )?
    .unwrap_or(EmbeddingConfig::DEFAULT_TIMEOUT_SECS);
    let endpoint = resolve_string(
        None,
        "ILEARNED_EMBED_ENDPOINT",
        file.and_then(|f| f.endpoint.clone()),
    )?;
    let model = resolve_string(
        None,
        "ILEARNED_EMBED_MODEL",
        file.and_then(|f| f.model.clone()),
    )?;
    let api_key = resolve_string(
        None,
        "ILEARNED_EMBED_API_KEY",
        file.and_then(|f| f.api_key.clone()),
    )?;
    if endpoint.is_none() && model.is_none() && api_key.is_none() {
        return Ok(None);
    }
    Ok(Some(ResolvedEmbeddingConfig {
        endpoint,
        model,
        api_key_configured: api_key.is_some(),
        dims,
        timeout_secs,
    }))
}

fn configured_database_path(
    explicit: Option<PathBuf>,
    file: Option<&FileConfig>,
) -> Option<PathBuf> {
    explicit
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| {
            std::env::var_os("ILEARNED_DB")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            file.and_then(|f| f.db.clone())
                .filter(|path| !path.as_os_str().is_empty())
        })
}

fn resolve_database_path(explicit: Option<PathBuf>, file: Option<&FileConfig>) -> Option<PathBuf> {
    configured_database_path(explicit, file).or_else(|| {
        let local = FileConfig::local_db_fallback();
        if local.is_file() {
            Some(local)
        } else {
            FileConfig::global_data_db_path().filter(|path| path.is_file())
        }
    })
}

fn resolve_bind(
    bind: Option<SocketAddr>,
    file: Option<&FileConfig>,
) -> Result<SocketAddr, AppError> {
    if let Some(bind) = bind {
        return Ok(bind);
    }
    if let Some(bind) = parse_env("ILEARNED_BIND")? {
        return Ok(bind);
    }
    match file.and_then(|f| f.bind.clone()) {
        Some(s) => s
            .parse()
            .map_err(|e| AppError::InvalidInput(format!("invalid bind in config file {s:?}: {e}"))),
        None => Ok("127.0.0.1:8787".parse().expect("default bind parses")),
    }
}

fn resolve_lifecycle(
    active_days: Option<u64>,
    forget_days: Option<u64>,
    retention_days: Option<u64>,
    file: Option<&FileConfig>,
) -> Result<LifecycleConfig, AppError> {
    let file_days = |pick: fn(&FileConfig) -> Option<u64>| file.and_then(pick);
    let defaults = LifecycleConfig::default();
    let auto_delete_threshold = match file.and_then(|f| f.auto_delete_threshold) {
        Some(t) if !(0.0..=1.0).contains(&t) => {
            return Err(AppError::InvalidInput(format!(
                "auto_delete_threshold must be within 0.0..=1.0, got {t}"
            )));
        }
        t => t,
    };
    Ok(LifecycleConfig {
        active_period_days: resolve_setting(
            active_days,
            "ILEARNED_ACTIVE_DAYS",
            file_days(|f| f.active_days),
        )?
        .unwrap_or(defaults.active_period_days),
        forget_period_days: resolve_setting(
            forget_days,
            "ILEARNED_FORGET_DAYS",
            file_days(|f| f.forget_days),
        )?
        .unwrap_or(defaults.forget_period_days),
        retention_days: resolve_setting(
            retention_days,
            "ILEARNED_RETENTION_DAYS",
            file_days(|f| f.retention_days),
        )?
        .unwrap_or(defaults.retention_days),
        auto_delete_threshold: auto_delete_threshold.unwrap_or(defaults.auto_delete_threshold),
    })
}

/// Runtime configuration.
/// Configured values use precedence: explicit values > `ILEARNED_*` env >
/// selected config overlay > local file > global file. Non-database settings
/// then use defaults; an unconfigured database uses existing fallback files or
/// returns an error.
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

    /// Env/file resolution with an explicit (already merged) file layer.
    /// Explicit values take precedence over env, then file values. Non-database
    /// settings use defaults; database resolution is handled above.
    pub fn load_with_files(
        db_path: Option<PathBuf>,
        bind: Option<SocketAddr>,
        active_days: Option<u64>,
        forget_days: Option<u64>,
        retention_days: Option<u64>,
        embedding: Option<EmbeddingConfig>,
        file: Option<FileConfig>,
    ) -> Result<Self, AppError> {
        // No configured database: probe the local project store, then the
        // global data store. Error instead of silently creating a default.
        let db_path = resolve_database_path(db_path, file.as_ref()).ok_or_else(|| {
            AppError::InvalidInput(
                "db path is not configured: set db in a config file or \
                 ILEARNED_DB, or create ./.ilearned/ilearned.db or the \
                 XDG data database"
                    .to_string(),
            )
        })?;
        let bind = resolve_bind(bind, file.as_ref())?;
        let lifecycle = resolve_lifecycle(active_days, forget_days, retention_days, file.as_ref())?;
        Ok(Self {
            db_path,
            bind,
            lifecycle,
            embedding,
        })
    }
}
