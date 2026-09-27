use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn bin() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"));
    assert!(p.exists(), "binary missing: {}", p.display());
    let _ = &mut p;
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"))
}

fn db_arg(dir: &TempDir) -> String {
    dir.path().join("t.db").to_string_lossy().to_string()
}

#[test]
fn add_search_json_roundtrip() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    let out = Command::new(bin())
        .args([
            "--db", &db, "--json", "add", "--topic", "rust", "--when", "w", "--if", "i", "--do",
            "d", "--check", "c",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["topic"], "rust");
    assert!(v["id"].as_str().is_some());

    let out = Command::new(bin())
        .args(["--db", &db, "--json", "search", "--topic", "rust"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
}

#[test]
fn destructive_requires_confirmation() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    let out = Command::new(bin())
        .args([
            "--db", &db, "--json", "add", "--topic", "t", "--when", "w", "--if", "i", "--do", "d",
            "--check", "c",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["id"].as_str().unwrap().to_string();

    // Answer "n" to the prompt: non-zero exit, record still present.
    let mut child = Command::new(bin())
        .args(["--db", &db, "delete", "--topic", "t", "--id", &id])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    let out = Command::new(bin())
        .args(["--db", &db, "--json", "search", "--topic", "t"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);

    // --yes goes through.
    let out = Command::new(bin())
        .args(["--db", &db, "delete", "--topic", "t", "--id", &id, "--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn clear_requires_topic_or_all() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    let out = Command::new(bin())
        .args(["--db", &db, "clear"])
        .output()
        .unwrap();
    // clap error => exit code 2.
    assert_eq!(out.status.code(), Some(2));
}
