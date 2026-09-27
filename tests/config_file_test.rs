use std::path::PathBuf;

use ilearned::config::{Config, EmbeddingConfig, FileConfig};

#[test]
fn file_config_parses_all_keys() {
    let fc: FileConfig = toml::from_str(
        r#"
db = "/tmp/x.db"
bind = "127.0.0.1:9999"
active_days = 10
forget_days = 20
retention_days = 30
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
fn config_load_prefers_local_over_global_over_default() {
    // No flags, no env (env vars must be unset for this test).
    for k in [
        "ILEARNED_DB",
        "ILEARNED_ACTIVE_DAYS",
        "ILEARNED_FORGET_DAYS",
        "ILEARNED_RETENTION_DAYS",
    ] {
        unsafe { std::env::remove_var(k) };
    }
    let global: FileConfig = toml::from_str(r#"active_days = 10"#).unwrap();
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
    unsafe { std::env::set_var("XDG_CONFIG_HOME", xdg.path()) };
    let empty_cwd = tempfile::TempDir::new().unwrap();
    let _guard = CwdGuard::lock(empty_cwd.path());
    let res = FileConfig::load_files();
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    assert!(
        matches!(res, Err(ilearned::AppError::InvalidInput(_))),
        "expected InvalidInput, got {res:?}"
    );
}

/// Serializes CWD changes: capturing the original dir on lock and restoring
/// on drop keeps parallel tests from stranding the process elsewhere.
struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    fn lock(dir: &std::path::Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        Self { original }
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
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"));
    for k in ["ILEARNED_DB", "XDG_CONFIG_HOME"] {
        unsafe { std::env::remove_var(k) };
    }
    let out = std::process::Command::new(&bin)
        .args(["--json", "search", "--topic", "nope"])
        .current_dir(dir.path())
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
fn embedding_file_values_used_when_no_flag_or_env() {
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
    let emb = EmbeddingConfig::from_parts_with_files(None, None, None, None, None, fc.embedding);
    assert!(emb.is_some(), "file embedding config should resolve");
}
