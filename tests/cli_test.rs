use std::io::Write;
use std::process::{Command, Stdio};

use clap::Parser;
use ilearned::application::MemoryService;
use ilearned::domain::commands::AddCommand;
use ilearned::domain::lifecycle::LifecycleConfig;
use ilearned::embedding::FakeEmbeddingProvider;
use ilearned::storage::SqliteRepo;
use ilearned::surfaces::cli::commands::{Commands, EmbeddingCommands};
use ilearned::surfaces::cli::{run_cli, Cli};
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
            "modified": {"topic": "t", "id": id, "good_count": 2, "bad_count": 0, "state": "active"}
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
            "modified": {"topic": "t", "id": id, "good_count": 2, "bad_count": 1, "state": "active"}
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
fn demote_auto_delete_reports_state_and_hides_from_search() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("t.db");
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "db = {:?}\nauto_delete_threshold = 1.0\n",
            db_path.to_string_lossy()
        ),
    )
    .unwrap();
    let db = config.to_string_lossy().to_string();

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

    // Threshold 1.0: first demote (score 1/2 < 1.0) auto-deletes.
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
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "modified": {"topic": "t", "id": id, "good_count": 1, "bad_count": 1, "state": "deleted"}
        })
    );

    // Auto-deleted records are hidden from search.
    let out = Command::new(bin())
        .args(["--config-file", &db, "search", "--json", "--topic", "t"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 0);
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

#[test]
fn embedding_migrate_parses_prune_yes_json() {
    let cli = Cli::try_parse_from([
        "ilearned",
        "embedding",
        "migrate",
        "--prune",
        "--yes",
        "--json",
    ])
    .unwrap();
    match cli.command {
        Commands::Embedding(args) => match args.command {
            EmbeddingCommands::Migrate(args) => {
                assert!(args.prune);
                assert!(args.yes);
                assert!(args.output.json);
            }
        },
        other => panic!("expected embedding command, got {other:?}"),
    }
}

#[test]
fn embedding_migrate_json_renders_summary() {
    let dir = TempDir::new().unwrap();
    let repo = SqliteRepo::open(&dir.path().join("migrate.db")).unwrap();
    let svc = MemoryService::new(repo, LifecycleConfig::default())
        .with_embedding_provider(FakeEmbeddingProvider::new());
    svc.add(AddCommand {
        topic: "migration".to_string(),
        when_text: "when".to_string(),
        if_text: "if".to_string(),
        do_text: "do".to_string(),
        check_text: "check".to_string(),
    })
    .unwrap();
    let cli = Cli::try_parse_from(["ilearned", "embedding", "migrate", "--json"]).unwrap();
    let output = run_cli(&svc, &cli.command, true).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value["embedding_migration"]["model"], "fake-test");
    assert_eq!(value["embedding_migration"]["dims"], 64);
    assert_eq!(value["embedding_migration"]["total"], 1);
    assert_eq!(value["embedding_migration"]["migrated"], 1);
    assert_eq!(value["embedding_migration"]["pruned"], 0);
}

#[test]
fn embedding_migrate_requires_provider() {
    let dir = TempDir::new().unwrap();
    let config = db_arg(&dir);
    let output = Command::new(bin())
        .args(["--config-file", &config, "embedding", "migrate", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("no embedding provider"));
}

#[test]
fn embedding_migrate_refuses_prune_without_confirmation() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("t.db");
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "db = {:?}\n[embedding]\nendpoint = \"http://127.0.0.1:1/v1\"\nmodel = \"test-model\"\napi_key = \"test-key\"\ndims = 64\n",
            db.to_string_lossy()
        ),
    )
    .unwrap();
    let mut child = Command::new(bin())
        .args([
            "--config-file",
            config.to_str().unwrap(),
            "embedding",
            "migrate",
            "--prune",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"n\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not confirmed"));
}

#[test]
fn db_encrypt_converts_plaintext_database_and_hides_key() {
    let dir = TempDir::new().unwrap();
    let config = db_arg(&dir);
    let db = dir.path().join("t.db");
    let key = "test database key";

    let add = Command::new(bin())
        .env_remove("ILEARNED_DB_KEY")
        .args([
            "--config-file",
            &config,
            "add",
            "--topic",
            "encrypted",
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
        add.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert_eq!(&std::fs::read(&db).unwrap()[..16], b"SQLite format 3\0");

    let output = Command::new(bin())
        .env("ILEARNED_DB_KEY", key)
        .args(["--config-file", &config, "db", "encrypt", "--yes", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({"encrypted": {"path": db.to_string_lossy()}})
    );
    assert_ne!(&std::fs::read(&db).unwrap()[..16], b"SQLite format 3\0");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(key));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(key));

    let search = Command::new(bin())
        .env("ILEARNED_DB_KEY", key)
        .args([
            "--config-file",
            &config,
            "search",
            "--json",
            "--topic",
            "encrypted",
        ])
        .output()
        .unwrap();
    assert!(
        search.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&search.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn db_encrypt_refuses_unconfirmed_conversion_without_mutating_source() {
    let dir = TempDir::new().unwrap();
    let config = db_arg(&dir);
    let db = dir.path().join("t.db");
    let key = "confirmation key";
    assert!(Command::new(bin())
        .env_remove("ILEARNED_DB_KEY")
        .args([
            "--config-file",
            &config,
            "add",
            "--topic",
            "plain",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .status()
        .unwrap()
        .success());
    let before = std::fs::read(&db).unwrap();

    let mut child = Command::new(bin())
        .env("ILEARNED_DB_KEY", key)
        .args(["--config-file", &config, "db", "encrypt"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn db_encrypt_refuses_missing_key_without_mutating_source() {
    let dir = TempDir::new().unwrap();
    let config = db_arg(&dir);
    let db = dir.path().join("t.db");
    assert!(Command::new(bin())
        .env_remove("ILEARNED_DB_KEY")
        .args([
            "--config-file",
            &config,
            "add",
            "--topic",
            "plain",
            "--when",
            "w",
            "--if",
            "i",
            "--do",
            "d",
            "--check",
            "c",
        ])
        .status()
        .unwrap()
        .success());
    let before = std::fs::read(&db).unwrap();

    let output = Command::new(bin())
        .env_remove("ILEARNED_DB_KEY")
        .args(["--config-file", &config, "db", "encrypt", "--yes"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(std::fs::read(&db).unwrap(), before);
}

#[test]
fn db_encrypt_keyed_new_database_supports_crud_and_wrong_key_fails_without_leaking_it() {
    let dir = TempDir::new().unwrap();
    let config = db_arg(&dir);
    let key = "correct database key";
    let wrong_key = "wrong database key";

    let add = Command::new(bin())
        .env("ILEARNED_DB_KEY", key)
        .args([
            "--config-file",
            &config,
            "add",
            "--topic",
            "keyed",
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
        add.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let search = Command::new(bin())
        .env("ILEARNED_DB_KEY", key)
        .args([
            "--config-file",
            &config,
            "search",
            "--json",
            "--topic",
            "keyed",
        ])
        .output()
        .unwrap();
    assert!(
        search.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&search.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let wrong = Command::new(bin())
        .env("ILEARNED_DB_KEY", wrong_key)
        .args(["--config-file", &config, "search", "--topic", "keyed"])
        .output()
        .unwrap();
    assert_eq!(wrong.status.code(), Some(4));
    assert!(!String::from_utf8_lossy(&wrong.stderr).contains(wrong_key));
}
