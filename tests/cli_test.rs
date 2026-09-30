use std::io::Write;
use std::process::{Command, Stdio};

use clap::Parser;
use ilearned::surfaces::cli::Cli;
use tempfile::TempDir;

fn bin() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"));
    assert!(p.exists(), "binary missing: {}", p.display());
    let _ = &mut p;
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"))
}

fn db_arg(dir: &TempDir) -> String {
    let db = dir.path().join("t.db");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("db = {:?}\n", db.to_string_lossy())).unwrap();
    config.to_string_lossy().to_string()
}

#[test]
fn add_search_json_roundtrip() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "add",
            "--json",
            "--topic",
            "rust",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["added"]["topic"], "rust");
    assert!(v["added"]["id"].as_str().is_some());

    let out = Command::new(bin())
        .args(["--config-file", &db, "search", "--json", "--topic", "rust"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
}

#[test]
fn topic_list_and_search_json() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    for topic in ["travel/hotel/checkout", "travel/flight"] {
        let out = Command::new(bin())
            .args([
                "--config-file",
                &db,
                "add",
                "--json",
                "--topic",
                topic,
                "--when",
                "w",
                "--if",
                "i",
                "--do",
                "d",
                "--check",
                "c",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // list --level 1 collapses to the shared prefix.
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "topic",
            "list",
            "--json",
            "--level",
            "1",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v, serde_json::json!(["travel"]));
    // search with a # pattern returns the full topics sorted.
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "topic",
            "search",
            "--json",
            "travel/#",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v,
        serde_json::json!(["travel/flight", "travel/hotel/checkout"])
    );
}

#[test]
fn destructive_requires_confirmation() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "add",
            "--json",
            "--topic",
            "t",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["added"]["id"].as_str().unwrap().to_string();

    // Answer "n" to the prompt: non-zero exit, record still present.
    let mut child = Command::new(bin())
        .args(["--config-file", &db, "delete", "--topic", "t", "--id", &id])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    let out = Command::new(bin())
        .args(["--config-file", &db, "search", "--json", "--topic", "t"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);

    // --yes goes through.
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "delete",
            "--topic",
            "t",
            "--id",
            &id,
            "--yes",
        ])
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
        .args(["--config-file", &db, "clear"])
        .output()
        .unwrap();
    // clap error => exit code 2.
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn clear_accepts_yes_with_target() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "add",
            "--json",
            "--topic",
            "t",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());

    // --topic combined with --yes must be accepted (not a clap conflict).
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "clear",
            "--json",
            "--topic",
            "t",
            "--yes",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clear --topic --yes rejected: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "add",
            "--json",
            "--topic",
            "t",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());

    // --all combined with --yes must be accepted (not a clap conflict).
    let out = Command::new(bin())
        .args(["--config-file", &db, "clear", "--json", "--all", "--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clear --all --yes rejected: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn mutation_json_outputs_only_identity_or_feedback_counts() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "add",
            "--json",
            "--topic",
            "t",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let added: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = added["added"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        added,
        serde_json::json!({"added": {"topic": "t", "id": id}})
    );

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "update",
            "--json",
            "--topic",
            "t",
            "--id",
            &id,
            "--when",
            "updated",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let modified: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        modified,
        serde_json::json!({"modified": {"topic": "t", "id": id}})
    );

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "promote",
            "--json",
            "--topic",
            "t",
            "--id",
            &id,
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let promoted: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        promoted,
        serde_json::json!({
            "modified": {"topic": "t", "id": id, "good_count": 2, "bad_count": 0}
        })
    );

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "demote",
            "--json",
            "--topic",
            "t",
            "--id",
            &id,
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let downgraded: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        downgraded,
        serde_json::json!({
            "modified": {"topic": "t", "id": id, "good_count": 2, "bad_count": 1}
        })
    );

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "delete",
            "--json",
            "--yes",
            "--topic",
            "t",
            "--id",
            &id,
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let deleted: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        deleted,
        serde_json::json!({"deleted": {"topic": "t", "id": id}})
    );
}

#[test]
fn clear_json_reports_removed_topics_and_items() {
    let dir = TempDir::new().unwrap();
    let db = db_arg(&dir);

    for topic in ["a", "a", "b"] {
        let out = Command::new(bin())
            .args([
                "--config-file",
                &db,
                "add",
                "--json",
                "--topic",
                topic,
                "--when",
                "w",
                "--if",
                "i",
                "--do",
                "d",
                "--check",
                "c",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
    }

    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "delete",
            "--yes",
            "--topic",
            "b",
            "--id",
            "missing",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());

    let out = Command::new(bin())
        .args(["--config-file", &db, "search", "--json", "--topic", "b"])
        .output()
        .unwrap();
    let records: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let b_id = records[0]["id"].as_str().unwrap().to_string();
    let out = Command::new(bin())
        .args([
            "--config-file",
            &db,
            "delete",
            "--yes",
            "--topic",
            "b",
            "--id",
            &b_id,
        ])
        .output()
        .unwrap();
    assert!(out.status.success());

    let out = Command::new(bin())
        .args(["--config-file", &db, "clear", "--json", "--all", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let cleared: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        cleared,
        serde_json::json!({"cleared": {"num_of_topics": 1, "num_of_items": 2}})
    );
}

#[test]
fn json_is_rejected_for_protocol_and_jsonl_commands() {
    for args in [
        vec!["ilearned", "serve", "--json"],
        vec!["ilearned", "mcp", "--json"],
        vec!["ilearned", "export", "--json"],
    ] {
        assert!(
            Cli::try_parse_from(args.clone()).is_err(),
            "--json unexpectedly accepted for {:?}",
            args
        );
    }
}
