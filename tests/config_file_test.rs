use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use ilearned::config::{Config, EmbeddingConfig, FileConfig};

static PROCESS_ENV_LOCK: Mutex<()> = Mutex::new(());
const CONFIG_ENV_VARS: [&str; 14] = [
    "ILEARNED_DB",
    "ILEARNED_DB_KEY",
    "ILEARNED_BIND",
    "ILEARNED_ACTIVE_DAYS",
    "ILEARNED_FORGET_DAYS",
    "ILEARNED_RETENTION_DAYS",
    "ILEARNED_EMBED_ENDPOINT",
    "ILEARNED_EMBED_MODEL",
    "ILEARNED_EMBED_API_KEY",
    "ILEARNED_EMBED_DIMS",
    "ILEARNED_EMBED_TIMEOUT_SECS",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "HOME",
];

fn isolated_command(dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ilearned"));
    for name in CONFIG_ENV_VARS {
        command.env_remove(name);
    }
    command
        .env("XDG_CONFIG_HOME", dir.join("empty-xdg-config"))
        .current_dir(dir);
    command
}

struct EnvGuard {
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn capture() -> Self {
        Self {
            previous: CONFIG_ENV_VARS
                .into_iter()
                .map(|name| (name, std::env::var_os(name)))
                .collect(),
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, value) in &self.previous {
            match value {
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
    }
}

#[test]
fn file_config_parses_all_keys() {
    let fc: FileConfig = toml::from_str(
        r#"
db = "/tmp/x.db"
bind = "127.0.0.1:9999"
active_days = 10
forget_days = 20
retention_days = 30
auto_delete_threshold = 0.5
[embedding]
endpoint = "http://localhost:11434/v1"
model = "nomic-embed"
api_key = "secret"
dims = 768
timeout_secs = 5
"#,
    )
    .expect("valid file config parses");
    assert_eq!(fc.db, Some(PathBuf::from("/tmp/x.db")));
    assert_eq!(fc.active_days, Some(10));
    assert_eq!(fc.auto_delete_threshold, Some(0.5));
    let emb = fc.embedding.expect("embedding parses");
    assert_eq!(emb.model.as_deref(), Some("nomic-embed"));
    assert_eq!(emb.dims, Some(768));
}

#[test]
fn file_config_rejects_unknown_keys() {
    let res: Result<FileConfig, _> = toml::from_str(r#"bogus_key = 1"#);
    assert!(res.is_err(), "unknown keys must be rejected");
}

#[test]
fn auto_delete_threshold_out_of_range_is_rejected() {
    let dir = tempfile::TempDir::new().unwrap();
    let config = dir.path().join("bad-threshold.toml");
    std::fs::write(&config, "auto_delete_threshold = 1.5\n").unwrap();
    let out = isolated_command(dir.path())
        .args(["--config-file", config.to_str().unwrap(), "config", "show"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "out-of-range threshold must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("auto_delete_threshold"),
        "stderr should name the setting: {stderr}"
    );
}

#[test]
fn auto_delete_threshold_merges_per_field() {
    let global: FileConfig = toml::from_str("auto_delete_threshold = 0.1").unwrap();
    let local: FileConfig = toml::from_str("auto_delete_threshold = 0.4").unwrap();
    let merged = FileConfig::merge(Some(global), Some(local)).expect("merge yields config");
    assert_eq!(merged.auto_delete_threshold, Some(0.4));
    let global_only: FileConfig = toml::from_str("auto_delete_threshold = 0.1").unwrap();
    let merged_keep = FileConfig::merge(Some(global_only), None).expect("merge yields config");
    assert_eq!(merged_keep.auto_delete_threshold, Some(0.1));
}

#[test]
fn local_file_overlays_global_per_field() {
    let global: FileConfig = toml::from_str(
        r#"
db = "/tmp/global.db"
active_days = 10
forget_days = 20
"#,
    )
    .unwrap();
    let local: FileConfig = toml::from_str(
        r#"
active_days = 99
"#,
    )
    .unwrap();
    let merged = FileConfig::merge(Some(global), Some(local)).expect("merge yields config");
    assert_eq!(merged.db, Some(PathBuf::from("/tmp/global.db")));
    assert_eq!(merged.active_days, Some(99));
    assert_eq!(merged.forget_days, Some(20));
}

#[test]
fn explicit_file_overlays_local_and_global_per_field() {
    let global: FileConfig = toml::from_str(
        r#"
db = "/tmp/global.db"
bind = "127.0.0.1:9000"
active_days = 10
[embedding]
endpoint = "http://global.example/v1"
dims = 256
"#,
    )
    .unwrap();
    let local: FileConfig = toml::from_str(
        r#"
bind = "127.0.0.1:9001"
forget_days = 20
[embedding]
model = "local-model"
"#,
    )
    .unwrap();
    let explicit: FileConfig = toml::from_str(
        r#"
db = "/tmp/explicit.db"
retention_days = 30
[embedding]
timeout_secs = 5
"#,
    )
    .unwrap();

    let merged = FileConfig::merge(FileConfig::merge(Some(global), Some(local)), Some(explicit))
        .expect("all file layers should merge");
    assert_eq!(merged.db, Some(PathBuf::from("/tmp/explicit.db")));
    assert_eq!(merged.bind.as_deref(), Some("127.0.0.1:9001"));
    assert_eq!(merged.active_days, Some(10));
    assert_eq!(merged.forget_days, Some(20));
    assert_eq!(merged.retention_days, Some(30));
    let embedding = merged.embedding.expect("embedding layers should merge");
    assert_eq!(
        embedding.endpoint.as_deref(),
        Some("http://global.example/v1")
    );
    assert_eq!(embedding.model.as_deref(), Some("local-model"));
    assert_eq!(embedding.dims, Some(256));
    assert_eq!(embedding.timeout_secs, Some(5));
}

#[test]
fn config_load_prefers_local_over_global_over_default() {
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    // No explicit values, no env (env vars must be unset for this test).
    for k in [
        "ILEARNED_DB",
        "ILEARNED_ACTIVE_DAYS",
        "ILEARNED_FORGET_DAYS",
        "ILEARNED_RETENTION_DAYS",
    ] {
        unsafe { std::env::remove_var(k) };
    }
    let global: FileConfig = toml::from_str(
        r#"db = "/tmp/x.db"
active_days = 10"#,
    )
    .unwrap();
    let local: FileConfig = toml::from_str(r#"active_days = 99"#).unwrap();
    let merged = FileConfig::merge(Some(global), Some(local));
    let cfg = Config::load_with_files(None, None, None, None, None, None, merged).unwrap();
    assert_eq!(cfg.lifecycle.active_period_days, 99);
}

#[test]
fn malformed_global_file_fails_typed() {
    // A syntactically invalid global file must surface InvalidInput,
    // not silently fall back to defaults. Uses an isolated XDG dir plus
    // a CWD guaranteed to have no local file.
    let xdg = tempfile::TempDir::new().unwrap();
    let ilearned_dir = xdg.path().join("ilearned");
    std::fs::create_dir_all(&ilearned_dir).unwrap();
    std::fs::write(ilearned_dir.join("config.toml"), "db = [unclosed\n").unwrap();
    let empty_cwd = tempfile::TempDir::new().unwrap();
    let _guard = CwdGuard::lock(empty_cwd.path());
    let _env = EnvGuard::capture();
    unsafe { std::env::set_var("XDG_CONFIG_HOME", xdg.path()) };
    let res = FileConfig::load_files();
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    assert!(
        matches!(res, Err(ilearned::AppError::InvalidInput(_))),
        "expected InvalidInput, got {res:?}"
    );
}

/// Serializes process-global CWD and environment changes while restoring CWD
/// on drop, so parallel tests cannot strand the process elsewhere.
struct CwdGuard {
    original: PathBuf,
    _lock: MutexGuard<'static, ()>,
}

impl CwdGuard {
    fn lock(dir: &std::path::Path) -> Self {
        let lock = PROCESS_ENV_LOCK.lock().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        Self {
            original,
            _lock: lock,
        }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

#[test]
fn end_to_end_local_file_sets_db() {
    // Local ./.ilearned/config.toml is honored by the real binary:
    // `search` against a file-configured db path must succeed.
    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("file.db");
    let ilearned_dir = dir.path().join(".ilearned");
    std::fs::create_dir_all(&ilearned_dir).unwrap();
    std::fs::write(
        ilearned_dir.join("config.toml"),
        format!("db = {:?}\n", db.to_string_lossy()),
    )
    .unwrap();
    let out = isolated_command(dir.path())
        .args(["search", "--json", "--topic", "nope"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 0);
    assert!(
        db.exists(),
        "db should be created at the file-configured path"
    );
}

#[test]
fn cli_environment_database_overrides_config_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let file_db = dir.path().join("file.db");
    let env_db = dir.path().join("env.db");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("db = {:?}\n", file_db.to_string_lossy())).unwrap();

    let out = isolated_command(dir.path())
        .args([
            "--config-file",
            config.to_str().unwrap(),
            "search",
            "--json",
            "--topic",
            "nope",
        ])
        .env("ILEARNED_DB", &env_db)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(env_db.exists(), "the environment database should be used");
    assert!(!file_db.exists(), "the file database should be overridden");
}

#[cfg(unix)]
#[test]
fn non_utf8_database_environment_overrides_config_file() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = tempfile::TempDir::new().unwrap();
    let file_db = dir.path().join("file.db");
    let env_db = dir.path().join(OsString::from_vec(b"env-\xff.db".to_vec()));
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("db = {:?}\n", file_db.to_string_lossy())).unwrap();

    let out = isolated_command(dir.path())
        .args([
            "--config-file",
            config.to_str().unwrap(),
            "search",
            "--json",
            "--topic",
            "nope",
        ])
        .env("ILEARNED_DB", env_db.as_os_str())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        env_db.exists(),
        "the non-UTF-8 environment path should be used"
    );
    assert!(!file_db.exists(), "the file database should be overridden");
}

#[test]
fn embedding_file_values_used_when_no_flag_or_env() {
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    for k in [
        "ILEARNED_EMBED_ENDPOINT",
        "ILEARNED_EMBED_MODEL",
        "ILEARNED_EMBED_API_KEY",
        "ILEARNED_EMBED_DIMS",
        "ILEARNED_EMBED_TIMEOUT_SECS",
    ] {
        unsafe { std::env::remove_var(k) };
    }
    let fc: FileConfig = toml::from_str(
        r#"
[embedding]
endpoint = "http://localhost:11434/v1"
model = "m"
api_key = "k"
"#,
    )
    .unwrap();
    let emb = EmbeddingConfig::from_parts_with_files(None, None, None, None, None, fc.embedding)
        .expect("file embedding config should parse");
    assert!(emb.is_some(), "file embedding config should resolve");
}

#[test]
fn environment_values_override_selected_config_file() {
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    for k in [
        "ILEARNED_DB",
        "ILEARNED_BIND",
        "ILEARNED_ACTIVE_DAYS",
        "ILEARNED_FORGET_DAYS",
        "ILEARNED_RETENTION_DAYS",
        "ILEARNED_EMBED_ENDPOINT",
        "ILEARNED_EMBED_MODEL",
        "ILEARNED_EMBED_API_KEY",
        "ILEARNED_EMBED_DIMS",
        "ILEARNED_EMBED_TIMEOUT_SECS",
    ] {
        unsafe { std::env::remove_var(k) };
    }
    let file: FileConfig = toml::from_str(
        r#"
db = "/tmp/file.db"
bind = "127.0.0.1:10001"
active_days = 1
forget_days = 2
retention_days = 3
[embedding]
endpoint = "http://file.example/v1"
model = "file-model"
api_key = "file-key"
dims = 1
timeout_secs = 2
"#,
    )
    .unwrap();
    unsafe {
        std::env::set_var("ILEARNED_DB", "/tmp/env.db");
        std::env::set_var("ILEARNED_BIND", "127.0.0.1:10002");
        std::env::set_var("ILEARNED_ACTIVE_DAYS", "11");
        std::env::set_var("ILEARNED_FORGET_DAYS", "12");
        std::env::set_var("ILEARNED_RETENTION_DAYS", "13");
        std::env::set_var("ILEARNED_EMBED_ENDPOINT", "http://env.example/v1");
        std::env::set_var("ILEARNED_EMBED_MODEL", "env-model");
        std::env::set_var("ILEARNED_EMBED_API_KEY", "env-key");
        std::env::set_var("ILEARNED_EMBED_DIMS", "11");
        std::env::set_var("ILEARNED_EMBED_TIMEOUT_SECS", "12");
    }

    let cfg = Config::load_with_files(None, None, None, None, None, None, Some(file.clone()))
        .expect("valid environment values should resolve");
    assert_eq!(cfg.db_path, PathBuf::from("/tmp/env.db"));
    assert_eq!(cfg.bind.to_string(), "127.0.0.1:10002");
    assert_eq!(cfg.lifecycle.active_period_days, 11);
    assert_eq!(cfg.lifecycle.forget_period_days, 12);
    assert_eq!(cfg.lifecycle.retention_days, 13);

    let embedding =
        EmbeddingConfig::from_parts_with_files(None, None, None, None, None, file.embedding)
            .expect("valid embedding environment values should resolve")
            .expect("embedding endpoint/model/key should enable provider");
    assert_eq!(embedding.endpoint, "http://env.example/v1");
    assert_eq!(embedding.model, "env-model");
    assert_eq!(embedding.api_key, "env-key");
    assert_eq!(embedding.dims, 11);
    assert_eq!(embedding.timeout_secs, 12);

    for k in [
        "ILEARNED_DB",
        "ILEARNED_BIND",
        "ILEARNED_ACTIVE_DAYS",
        "ILEARNED_FORGET_DAYS",
        "ILEARNED_RETENTION_DAYS",
        "ILEARNED_EMBED_ENDPOINT",
        "ILEARNED_EMBED_MODEL",
        "ILEARNED_EMBED_API_KEY",
        "ILEARNED_EMBED_DIMS",
        "ILEARNED_EMBED_TIMEOUT_SECS",
    ] {
        unsafe { std::env::remove_var(k) };
    }
}

#[test]
fn invalid_environment_values_are_rejected_instead_of_ignored() {
    let dir = tempfile::TempDir::new().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
db = "database.db"
bind = "127.0.0.1:8787"
active_days = 60
forget_days = 120
retention_days = 60
[embedding]
endpoint = "http://localhost:11434/v1"
model = "model"
api_key = "key"
"#,
    )
    .unwrap();

    for (name, value) in [
        ("ILEARNED_BIND", "not-a-socket"),
        ("ILEARNED_ACTIVE_DAYS", "not-a-number"),
        ("ILEARNED_FORGET_DAYS", "not-a-number"),
        ("ILEARNED_RETENTION_DAYS", "not-a-number"),
        ("ILEARNED_EMBED_DIMS", "not-a-number"),
        ("ILEARNED_EMBED_TIMEOUT_SECS", "not-a-number"),
    ] {
        let out = isolated_command(dir.path())
            .args([
                "--config-file",
                config.to_str().unwrap(),
                "search",
                "--json",
                "--topic",
                "nope",
            ])
            .env(name, value)
            .output()
            .unwrap();
        assert!(
            !out.status.success(),
            "{name}={value} unexpectedly succeeded"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(name),
            "error should name {name}, got stderr: {stderr}"
        );
    }
}

#[test]
fn cli_config_file_overlays_local_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let local_db = dir.path().join("local.db");
    let override_db = dir.path().join("override.db");
    let override_file = dir.path().join("override.toml");
    std::fs::create_dir_all(dir.path().join(".ilearned")).unwrap();
    std::fs::write(
        dir.path().join(".ilearned/config.toml"),
        format!("db = {:?}\n", local_db.to_string_lossy()),
    )
    .unwrap();
    std::fs::write(
        &override_file,
        format!("db = {:?}\n", override_db.to_string_lossy()),
    )
    .unwrap();

    let out = isolated_command(dir.path())
        .args([
            "--config-file",
            override_file.to_str().unwrap(),
            "search",
            "--topic",
            "nope",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        override_db.exists(),
        "the override config database should be used"
    );
    assert!(
        !local_db.exists(),
        "the local config database should be overridden"
    );
}

#[test]
fn cli_help_exposes_config_file_but_not_removed_global_flags() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ilearned"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--config-file"));
    for removed in [
        "--db",
        "--bind",
        "--active-days",
        "--forget-days",
        "--retention-days",
        "--embed-endpoint",
        "--embed-model",
        "--embed-api-key",
        "--embed-dims",
        "--embed-timeout-secs",
        "--json",
    ] {
        assert!(!help.contains(removed), "root help still exposes {removed}");
    }
}

#[test]
fn removed_global_database_flag_is_rejected() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_ilearned"))
        .args(["--db", "/tmp/removed.db", "search", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unexpected argument"));
}

#[test]
fn missing_cli_config_file_is_rejected() {
    let dir = tempfile::TempDir::new().unwrap();
    let missing = dir.path().join("missing.toml");
    let out = isolated_command(dir.path())
        .args([
            "--config-file",
            missing.to_str().unwrap(),
            "search",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist"));
}

#[test]
fn config_show_reports_resolved_values_and_existing_sources_without_a_database() {
    let dir = tempfile::TempDir::new().unwrap();
    let global = dir.path().join("empty-xdg-config/ilearned/config.toml");
    let local = dir.path().join(".ilearned/config.toml");
    let explicit = dir.path().join("override.toml");
    let local_db = dir.path().join("local.db");
    let explicit_db = dir.path().join("explicit.db");
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, "forget_days = 22\n").unwrap();
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(
        &local,
        format!(
            "db = {:?}\nbind = \"127.0.0.1:9000\"\nactive_days = 10\n",
            local_db.to_string_lossy()
        ),
    )
    .unwrap();
    std::fs::write(
        &explicit,
        format!(
            "db = {:?}\nbind = \"127.0.0.1:9001\"\nactive_days = 11\n",
            explicit_db.to_string_lossy()
        ),
    )
    .unwrap();

    let out = isolated_command(dir.path())
        .args([
            "--config-file",
            explicit.to_str().unwrap(),
            "config",
            "show",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["config"]["db"],
        explicit_db.to_string_lossy().as_ref()
    );
    assert_eq!(value["config"]["bind"], "127.0.0.1:9001");
    assert_eq!(value["config"]["active_days"], 11);
    assert_eq!(value["config"]["forget_days"], 22);
    let files = value["config_files"].as_array().unwrap();
    assert!(
        files.iter().any(|path| path.as_str() == global.to_str()),
        "global config path missing from {files:?}"
    );
    assert!(
        files.iter().any(|path| path
            .as_str()
            .is_some_and(|path| path.ends_with(".ilearned/config.toml"))),
        "local config path missing from {files:?}"
    );
    assert!(
        files.iter().any(|path| path.as_str() == explicit.to_str()),
        "explicit config path missing from {files:?}"
    );
}

#[test]
fn config_show_works_before_any_database_or_config_file_exists() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = isolated_command(dir.path())
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(value["config"]["db"].is_null());
    assert_eq!(value["config"]["bind"], "127.0.0.1:8787");
    assert_eq!(value["config"]["active_days"], 60);
    assert_eq!(value["config"]["auto_delete_threshold"], 0.3);
    assert!(value["config_files"].as_array().unwrap().is_empty());
}

#[test]
fn config_show_does_not_print_embedding_api_keys() {
    let dir = tempfile::TempDir::new().unwrap();
    let config = dir.path().join("embedding.toml");
    std::fs::write(
        &config,
        r#"
[embedding]
endpoint = "http://localhost:11434/v1"
model = "nomic-embed-text"
api_key = "super-secret-value"
"#,
    )
    .unwrap();

    let out = isolated_command(dir.path())
        .args(["--config-file", config.to_str().unwrap(), "config", "show"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("super-secret-value"), "stdout: {stdout}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["config"]["embedding"]["api_key_configured"], true);
    assert!(value["config"]["embedding"]["api_key"].is_null());
}

#[test]
fn config_init_generates_local_defaults_without_packaged_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let target = dir.path().join(".ilearned/config.toml");
    let out = isolated_command(dir.path())
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "./.ilearned/config.toml"
    );
    let text = std::fs::read_to_string(&target).expect("local config should be created");
    let parsed: FileConfig = toml::from_str(&text).expect("generated config should be valid TOML");
    assert_eq!(
        parsed.db.as_deref(),
        Some(Path::new("./.ilearned/ilearned.db"))
    );
    assert_eq!(parsed.bind.as_deref(), Some("127.0.0.1:8787"));
    assert_eq!(parsed.active_days, Some(60));
    assert_eq!(parsed.forget_days, Some(120));
    assert_eq!(parsed.retention_days, Some(60));
    assert_eq!(parsed.auto_delete_threshold, Some(0.3));
    assert!(text.contains("[embedding]"));
    assert!(
        text.contains("db = \"./.ilearned/ilearned.db\""),
        "db line should be uncommented: {text}"
    );
    assert!(
        !text.contains("# db ="),
        "db line must not be commented: {text}"
    );
}

#[test]
fn config_init_global_uses_xdg_config_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let global = dir.path().join("empty-xdg-config/ilearned/config.toml");
    let out = isolated_command(dir.path())
        .env("HOME", dir.path().join("fake-home"))
        .args(["config", "init", "-g"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        global.to_string_lossy()
    );
    assert!(
        global.is_file(),
        "global config missing at {}",
        global.display()
    );
    let parsed: FileConfig = toml::from_str(&std::fs::read_to_string(global).unwrap()).unwrap();
    assert_eq!(parsed.active_days, Some(60));
}

#[test]
fn config_init_global_sets_db_to_xdg_data_home() {
    let dir = tempfile::TempDir::new().unwrap();
    let data_home = dir.path().join("xdg-data");
    let global = dir.path().join("empty-xdg-config/ilearned/config.toml");
    let out = isolated_command(dir.path())
        .env("XDG_DATA_HOME", &data_home)
        .args(["config", "init", "-g"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: FileConfig = toml::from_str(&std::fs::read_to_string(global).unwrap()).unwrap();
    let expected = data_home.join("ilearned/ilearned.db");
    assert_eq!(
        parsed.db.as_deref(),
        Some(expected.as_path()),
        "global init should set db to $XDG_DATA_HOME/ilearned/ilearned.db"
    );
}

#[test]
fn config_init_global_falls_back_to_home_local_share() {
    let dir = tempfile::TempDir::new().unwrap();
    let home = dir.path().join("fake-home");
    std::fs::create_dir_all(&home).unwrap();
    let global = dir.path().join("empty-xdg-config/ilearned/config.toml");
    let out = isolated_command(dir.path())
        .env("HOME", &home)
        .args(["config", "init", "-g"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: FileConfig = toml::from_str(&std::fs::read_to_string(global).unwrap()).unwrap();
    let expected = home.join(".local/share/ilearned/ilearned.db");
    assert_eq!(
        parsed.db.as_deref(),
        Some(expected.as_path()),
        "global init without XDG_DATA_HOME should fall back to ~/.local/share"
    );
}

#[test]
fn config_init_refuses_to_overwrite_and_reports_existing_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let target = dir.path().join(".ilearned/config.toml");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "db = \"keep-me.db\"\n").unwrap();

    let out = isolated_command(dir.path())
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("./.ilearned/config.toml"),
        "stderr: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        "db = \"keep-me.db\"\n"
    );
}

#[test]
fn config_init_force_overwrites_existing_file_with_long_and_short_flags() {
    let dir = tempfile::TempDir::new().unwrap();
    let target = dir.path().join(".ilearned/config.toml");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "db = \"keep-me.db\"\n").unwrap();

    let out = isolated_command(dir.path())
        .args(["config", "init", "--force"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "./.ilearned/config.toml"
    );
    let text = std::fs::read_to_string(&target).unwrap();
    assert_ne!(text, "db = \"keep-me.db\"\n");
    let parsed: FileConfig = toml::from_str(&text).expect("forced config should be valid TOML");
    assert_eq!(
        parsed.db.as_deref(),
        Some(Path::new("./.ilearned/ilearned.db"))
    );

    std::fs::write(&target, "db = \"replace-again.db\"\n").unwrap();
    let out = isolated_command(dir.path())
        .args(["config", "init", "-f"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        std::fs::read_to_string(target).unwrap(),
        "db = \"replace-again.db\"\n"
    );
}

#[cfg(unix)]
#[test]
fn config_init_creates_private_config_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::TempDir::new().unwrap();
    let out = isolated_command(dir.path())
        .args(["config", "init"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mode = std::fs::metadata(dir.path().join(".ilearned/config.toml"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn serve_bind_overrides_config_file_and_environment() {
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("serve.db");
    let config = dir.path().join("serve.toml");
    std::fs::write(
        &config,
        format!("db = {:?}\nbind = \"not-a-socket\"\n", db.to_string_lossy()),
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let bind = format!("127.0.0.1:{port}");

    let mut child = isolated_command(dir.path())
        .args([
            "--config-file",
            config.to_str().unwrap(),
            "serve",
            "--bind",
            &bind,
        ])
        .env("ILEARNED_BIND", "also-not-a-socket")
        .spawn()
        .unwrap();

    for _ in 0..100 {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("serve exited before using the subcommand bind: {status}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            child.kill().unwrap();
            child.wait().unwrap();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    let status = child.wait().unwrap();
    panic!("serve did not listen on --bind {bind}; exited with {status}");
}

#[tokio::test]
async fn serve_prints_endpoint_urls_after_bind() {
    use std::process::Stdio;
    use tokio::io::AsyncBufReadExt;

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("serve.db");
    let config = dir.path().join("serve.toml");
    std::fs::write(&config, format!("db = {:?}\n", db.to_string_lossy())).unwrap();

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ilearned"));
    for name in CONFIG_ENV_VARS {
        command.env_remove(name);
    }
    let mut child = command
        .args([
            "--config-file",
            config.to_str().unwrap(),
            "serve",
            "--bind",
            "127.0.0.1:0",
        ])
        .env("XDG_CONFIG_HOME", dir.path().join("empty-xdg-config"))
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut lines = tokio::io::BufReader::new(stdout).lines();

    let web = tokio::time::timeout(std::time::Duration::from_secs(15), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let rest = tokio::time::timeout(std::time::Duration::from_secs(15), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mcp = tokio::time::timeout(std::time::Duration::from_secs(15), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let port = web
        .strip_prefix("Web UI: http://127.0.0.1:")
        .and_then(|url| url.strip_suffix('/'))
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or_else(|| panic!("unexpected web URL: {web}"));
    let base = format!("http://127.0.0.1:{port}");
    assert_eq!(rest, format!("REST API: {base}/api/v1"));
    assert_eq!(mcp, format!("MCP: {base}/mcp"));

    child.kill().await.unwrap();
    child.wait().await.unwrap();
}

#[test]
fn unconfigured_db_uses_local_ilearned_db_when_present() {
    let dir = tempfile::TempDir::new().unwrap();
    let local_db = dir.path().join(".ilearned").join("ilearned.db");
    std::fs::create_dir_all(local_db.parent().unwrap()).unwrap();
    std::fs::write(&local_db, b"").unwrap();
    let _guard = CwdGuard::lock(dir.path());
    let _env = EnvGuard::capture();
    unsafe { std::env::remove_var("ILEARNED_DB") };
    let cfg = Config::load_with_files(None, None, None, None, None, None, None)
        .expect("existing local db should resolve");
    assert_eq!(cfg.db_path, PathBuf::from("./.ilearned/ilearned.db"));
}

#[test]
fn unconfigured_db_falls_back_to_global_data_db() {
    let cwd = tempfile::TempDir::new().unwrap();
    let data_home = tempfile::TempDir::new().unwrap();
    let global_db = data_home.path().join("ilearned").join("ilearned.db");
    std::fs::create_dir_all(global_db.parent().unwrap()).unwrap();
    std::fs::write(&global_db, b"").unwrap();
    let _guard = CwdGuard::lock(cwd.path());
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB");
        std::env::set_var("XDG_DATA_HOME", data_home.path());
    }
    let cfg = Config::load_with_files(None, None, None, None, None, None, None)
        .expect("existing global db should resolve");
    assert_eq!(cfg.db_path, global_db);
}

#[test]
fn unconfigured_db_falls_back_to_home_data_db_without_xdg_override() {
    let cwd = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    let global_db = home
        .path()
        .join(".local")
        .join("share")
        .join("ilearned")
        .join("ilearned.db");
    std::fs::create_dir_all(global_db.parent().unwrap()).unwrap();
    std::fs::write(&global_db, b"").unwrap();
    let _guard = CwdGuard::lock(cwd.path());
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", home.path());
    }
    let cfg = Config::load_with_files(None, None, None, None, None, None, None)
        .expect("existing HOME data db should resolve");
    assert_eq!(cfg.db_path, global_db);
}

#[test]
fn unconfigured_db_does_not_use_relative_global_path_without_home() {
    let cwd = tempfile::TempDir::new().unwrap();
    let misleading = cwd
        .path()
        .join(".local")
        .join("share")
        .join("ilearned")
        .join("ilearned.db");
    std::fs::create_dir_all(misleading.parent().unwrap()).unwrap();
    std::fs::write(&misleading, b"").unwrap();
    let _guard = CwdGuard::lock(cwd.path());
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("HOME");
    }
    let res = Config::load_with_files(None, None, None, None, None, None, None);
    assert!(
        matches!(res, Err(ilearned::AppError::InvalidInput(ref msg)) if msg.contains("not configured")),
        "expected an unconfigured-db error, got {res:?}"
    );
}

#[test]
fn unconfigured_db_ignores_directory_named_like_local_database() {
    let cwd = tempfile::TempDir::new().unwrap();
    let local_db = cwd.path().join(".ilearned").join("ilearned.db");
    std::fs::create_dir_all(&local_db).unwrap();
    let data_home = tempfile::TempDir::new().unwrap();
    let _guard = CwdGuard::lock(cwd.path());
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB");
        std::env::set_var("XDG_DATA_HOME", data_home.path());
    }
    let res = Config::load_with_files(None, None, None, None, None, None, None);
    assert!(
        matches!(res, Err(ilearned::AppError::InvalidInput(ref msg)) if msg.contains("not configured")),
        "expected an unconfigured-db error, got {res:?}"
    );
}

#[test]
fn unconfigured_db_errors_when_no_db_found() {
    let cwd = tempfile::TempDir::new().unwrap();
    let data_home = tempfile::TempDir::new().unwrap();
    let _guard = CwdGuard::lock(cwd.path());
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB");
        std::env::set_var("XDG_DATA_HOME", data_home.path());
    }
    let res = Config::load_with_files(None, None, None, None, None, None, None);
    match res {
        Err(ilearned::AppError::InvalidInput(msg)) => {
            assert!(msg.contains("not configured"), "unexpected message: {msg}")
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn configured_db_wins_over_existing_fallback_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let local_db = dir.path().join(".ilearned").join("ilearned.db");
    std::fs::create_dir_all(local_db.parent().unwrap()).unwrap();
    std::fs::write(&local_db, b"").unwrap();
    let configured = dir.path().join("configured.db");
    let file: FileConfig = toml::from_str(&format!("db = {configured:?}")).unwrap();
    let _guard = CwdGuard::lock(dir.path());
    let _env = EnvGuard::capture();
    unsafe { std::env::remove_var("ILEARNED_DB") };
    let cfg = Config::load_with_files(None, None, None, None, None, None, Some(file)).unwrap();
    assert_eq!(cfg.db_path, configured);
}

#[test]
fn empty_database_environment_is_ignored() {
    let dir = tempfile::TempDir::new().unwrap();
    let configured = dir.path().join("configured.db");
    let file: FileConfig = toml::from_str(&format!("db = {configured:?}")).unwrap();
    let _guard = CwdGuard::lock(dir.path());
    let _env = EnvGuard::capture();
    unsafe { std::env::set_var("ILEARNED_DB", "") };
    let cfg = Config::load_with_files(None, None, None, None, None, None, Some(file)).unwrap();
    assert_eq!(cfg.db_path, configured);
}

#[test]
fn empty_database_file_value_is_not_configured() {
    let cwd = tempfile::TempDir::new().unwrap();
    let data_home = tempfile::TempDir::new().unwrap();
    let home = tempfile::TempDir::new().unwrap();
    let file = FileConfig {
        db: Some(PathBuf::new()),
        ..FileConfig::default()
    };
    let _guard = CwdGuard::lock(cwd.path());
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB");
        std::env::set_var("XDG_DATA_HOME", data_home.path());
        std::env::set_var("HOME", home.path());
    }
    let res = Config::load_with_files(None, None, None, None, None, None, Some(file));
    assert!(
        matches!(res, Err(ilearned::AppError::InvalidInput(ref msg)) if msg.contains("not configured")),
        "expected an unconfigured-db error, got {res:?}"
    );
}

#[test]
fn database_key_is_none_when_not_configured() {
    let dir = tempfile::TempDir::new().unwrap();
    let file: FileConfig =
        toml::from_str(&format!("db = {:?}", dir.path().join("db.sqlite"))).unwrap();
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    unsafe {
        std::env::remove_var("ILEARNED_DB_KEY");
    }
    let config = Config::load_with_files(None, None, None, None, None, None, Some(file)).unwrap();
    assert_eq!(config.db_key, None);
}

#[test]
fn database_key_comes_from_environment() {
    let dir = tempfile::TempDir::new().unwrap();
    let file: FileConfig =
        toml::from_str(&format!("db = {:?}", dir.path().join("db.sqlite"))).unwrap();
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    unsafe {
        std::env::set_var("ILEARNED_DB_KEY", "correct horse");
    }
    let config = Config::load_with_files(None, None, None, None, None, None, Some(file)).unwrap();
    assert_eq!(config.db_key.as_deref(), Some("correct horse"));
}

#[test]
fn empty_database_key_is_rejected() {
    let dir = tempfile::TempDir::new().unwrap();
    let file: FileConfig =
        toml::from_str(&format!("db = {:?}", dir.path().join("db.sqlite"))).unwrap();
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    unsafe {
        std::env::set_var("ILEARNED_DB_KEY", "");
    }
    let result = Config::load_with_files(None, None, None, None, None, None, Some(file));
    assert!(
        matches!(result, Err(ilearned::AppError::InvalidInput(message)) if message.contains("ILEARNED_DB_KEY"))
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_database_key_is_rejected() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::TempDir::new().unwrap();
    let file: FileConfig =
        toml::from_str(&format!("db = {:?}", dir.path().join("db.sqlite"))).unwrap();
    let _lock = PROCESS_ENV_LOCK.lock().unwrap();
    let _env = EnvGuard::capture();
    unsafe {
        std::env::set_var("ILEARNED_DB_KEY", std::ffi::OsString::from_vec(vec![0xff]));
    }
    let result = Config::load_with_files(None, None, None, None, None, None, Some(file));
    assert!(
        matches!(result, Err(ilearned::AppError::InvalidInput(message)) if message.contains("ILEARNED_DB_KEY"))
    );
}

#[test]
fn config_show_does_not_print_database_key_from_environment() {
    let dir = tempfile::TempDir::new().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!("db = {:?}\n", dir.path().join("db.sqlite")),
    )
    .unwrap();
    let key = "database-secret-must-not-appear";

    let output = isolated_command(dir.path())
        .env("ILEARNED_DB_KEY", key)
        .args(["--config-file", config.to_str().unwrap(), "config", "show"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(key));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(key));
}
