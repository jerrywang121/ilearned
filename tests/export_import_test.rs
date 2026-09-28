use std::io::Write;
use std::process::{Command, Stdio};

use ilearned::application::{ImportOutcome, MemoryService};
use ilearned::domain::lifecycle::LifecycleConfig;
use ilearned::domain::{AddCommand, Experience, State};
use ilearned::storage::repository::ExperienceRepo;
use ilearned::storage::SqliteRepo;

fn svc() -> (tempfile::TempDir, MemoryService<SqliteRepo>) {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepo::open(&dir.path().join("t.db")).unwrap();
    (dir, MemoryService::new(repo, LifecycleConfig::default()))
}

fn add_cmd(topic: &str, when: &str) -> AddCommand {
    AddCommand {
        topic: topic.to_string(),
        when_text: when.to_string(),
        if_text: "alert fires".to_string(),
        do_text: "restart worker".to_string(),
        check_text: "health ok".to_string(),
    }
}

fn exp(topic: &str, id: &str, when: &str) -> Experience {
    Experience {
        topic: topic.to_string(),
        id: id.to_string(),
        when_text: when.to_string(),
        if_text: "i".to_string(),
        do_text: "d".to_string(),
        check_text: "c".to_string(),
        updated_at: chrono::Utc::now(),
        good_count: 1,
        bad_count: 0,
        state: State::Active,
    }
}

#[test]
fn export_hides_deleted_and_forgotten() {
    let (_d, s) = svc();
    let keep = s.add(add_cmd("t", "keep me")).unwrap();
    let deep_only = s.add(add_cmd("t", "inactive one")).unwrap();
    let gone = s.add(add_cmd("t", "deleted one")).unwrap();
    {
        let repo = s.repo();
        let mut e = repo.get("t", &deep_only.id).unwrap().unwrap();
        e.state = State::Inactive;
        repo.update(&e).unwrap();
        let mut e = repo.get("t", &gone.id).unwrap().unwrap();
        e.state = State::Deleted;
        repo.update(&e).unwrap();
    }
    let shallow = s.export(Some("t"), false).unwrap();
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].id, keep.id);
    let deep = s.export(Some("t"), true).unwrap();
    assert_eq!(deep.len(), 2);
    assert!(deep.iter().all(|e| e.state != State::Deleted));
}

#[test]
fn export_filters_by_topic() {
    let (_d, s) = svc();
    s.add(add_cmd("a", "for a")).unwrap();
    s.add(add_cmd("b", "for b")).unwrap();
    let out = s.export(Some("a"), false).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].topic, "a");
    let all = s.export(None, false).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn import_merge_upserts_by_topic_and_id() {
    let (_d, s) = svc();
    // New (topic, id) inserts verbatim and keeps its id.
    let outcome = s
        .import_record(exp("t", "deadbeef", "first"), true)
        .unwrap();
    assert_eq!(outcome, ImportOutcome::New);
    let got = s.get("t", "deadbeef").unwrap();
    assert_eq!(got.when_text, "first");
    // Same (topic, id) overwrites fields.
    let mut second = exp("t", "deadbeef", "second");
    second.good_count = 7;
    let outcome = s.import_record(second, true).unwrap();
    assert_eq!(outcome, ImportOutcome::Updated);
    let got = s.get("t", "deadbeef").unwrap();
    assert_eq!(got.when_text, "second");
    assert_eq!(got.good_count, 7);
}

#[test]
fn import_without_merge_assigns_fresh_ids() {
    let (_d, s) = svc();
    let a = s.import_record(exp("t", "deadbeef", "one"), false).unwrap();
    let b = s.import_record(exp("t", "deadbeef", "two"), false).unwrap();
    assert_eq!((a, b), (ImportOutcome::New, ImportOutcome::New));
    let all = s.export(Some("t"), false).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().all(|e| e.id != "deadbeef"));
    assert_ne!(all[0].id, all[1].id);
}

#[test]
fn import_rejects_blank_fields() {
    let (_d, s) = svc();
    let mut bad = exp("t", "abc123xy", "ok");
    bad.when_text = "   ".to_string();
    assert!(matches!(
        s.import_record(bad, true),
        Err(ilearned::AppError::InvalidInput(_))
    ));
    let mut bad_id = exp("t", "  ", "ok");
    let _ = &mut bad_id;
    assert!(matches!(
        s.import_record(exp("t", "  ", "ok"), true),
        Err(ilearned::AppError::InvalidInput(_))
    ));
}

fn bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"))
}

fn db_arg(dir: &tempfile::TempDir) -> String {
    dir.path().join("t.db").to_string_lossy().to_string()
}

fn add_json(db: &str, topic: &str, when: &str) {
    let out = Command::new(bin())
        .args([
            "--db", db, "--json", "add", "--topic", topic, "--when", when, "--if", "i", "--do",
            "d", "--check", "c",
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
fn cli_export_import_file_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = db_arg(&dir);
    add_json(&db, "t", "first lesson");
    add_json(&db, "t", "second lesson");
    let file = dir.path().join("backup.jsonl");
    let file_s = file.to_string_lossy().to_string();

    let out = Command::new(bin())
        .args(["--db", &db, "--json", "export", "--file", &file_s])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["exported"], 2);

    let out = Command::new(bin())
        .args(["--db", &db, "--json", "clear", "--all", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success());

    let out = Command::new(bin())
        .args([
            "--db", &db, "--json", "import", "--file", &file_s, "--merge",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // After `clear --all` the rows are soft-deleted, not purged, so a
    // --merge re-import overwrites them in place (updated, not new).
    assert_eq!(v["new"], 0);
    assert_eq!(v["updated"], 2);
    assert_eq!(v["errors"], 0);

    let out = Command::new(bin())
        .args(["--db", &db, "--json", "search", "--topic", "t"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn cli_export_stdout_is_jsonl() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = db_arg(&dir);
    add_json(&db, "t", "stream me");
    let out = Command::new(bin())
        .args(["--db", &db, "export"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let v: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(v["topic"], "t");
    assert_eq!(v["when"], "stream me");
}

#[test]
fn cli_import_counts_bad_lines_and_keeps_good_ones() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = db_arg(&dir);
    let good = serde_json::to_string(&exp("t", "good1234", "good line")).unwrap();
    std::fs::write(
        dir.path().join("in.jsonl"),
        format!("{good}\nnot json at all\n"),
    )
    .unwrap();
    let file_s = dir.path().join("in.jsonl").to_string_lossy().to_string();

    let out = Command::new(bin())
        .args([
            "--db", &db, "--json", "import", "--file", &file_s, "--merge",
        ])
        .output()
        .unwrap();
    // One bad line => exit 2 with per-line errors on stderr; the good
    // line still commits.
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("line 2"));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["new"], 1);
    assert_eq!(v["errors"], 1);

    let out = Command::new(bin())
        .args(["--db", &db, "--json", "search", "--topic", "t"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
}

#[test]
fn cli_import_reads_stdin_without_merge() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = db_arg(&dir);
    let line = serde_json::to_string(&exp("t", "file-id-1", "via stdin")).unwrap();
    let mut child = Command::new(bin())
        .args(["--db", &db, "--json", "import"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{line}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["new"], 1);
    // Without --merge the file id is replaced with a fresh one.
    let out = Command::new(bin())
        .args(["--db", &db, "--json", "search", "--topic", "t"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0]["id"], "file-id-1");
}
