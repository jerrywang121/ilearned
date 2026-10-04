use chrono::{Duration, Utc};
use ilearned::domain::{Experience, State};
use ilearned::storage::embeddings::VectorStore;
use ilearned::storage::repository::ExperienceRepo;
use ilearned::storage::{encrypt_database, SqliteRepo};

fn exp(topic: &str, id: &str, updated_days_ago: i64, state: State) -> Experience {
    Experience {
        topic: topic.to_string(),
        id: id.to_string(),
        when_text: "when deploy fails".to_string(),
        if_text: "trigger word alpha".to_string(),
        do_text: "restart the worker".to_string(),
        check_text: "health check passes".to_string(),
        updated_at: Utc::now() - Duration::days(updated_days_ago),
        good_count: 1,
        bad_count: 0,
        state,
    }
}

fn open_repo() -> (tempfile::TempDir, SqliteRepo) {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepo::open(&dir.path().join("t.db")).unwrap();
    (dir, repo)
}

#[test]
fn crud_roundtrip_unix_epoch() {
    let (_d, repo) = open_repo();
    let e = exp("rust", "a1", 0, State::Active);
    repo.insert(&e).unwrap();
    let got = repo.get("rust", "a1").unwrap().unwrap();
    // INTEGER epoch seconds: sub-second precision is truncated.
    assert_eq!(got.updated_at.timestamp(), e.updated_at.timestamp());
    assert_eq!(got.topic, "rust");
    assert_eq!(got.state, State::Active);
    assert!(repo.get("rust", "nope").unwrap().is_none());
}

#[test]
fn keyed_database_reopens_with_same_key_and_has_no_sqlite_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keyed.db");
    let repo = SqliteRepo::open_with_key(&path, Some("test-key")).unwrap();
    repo.insert(&exp("rust", "a1", 0, State::Active)).unwrap();

    let header = std::fs::read(&path).unwrap();
    assert_ne!(&header[..16], b"SQLite format 3\0");

    let reopened = SqliteRepo::open_with_key(&path, Some("test-key")).unwrap();
    assert!(reopened.get("rust", "a1").unwrap().is_some());
}

#[test]
fn wrong_key_is_a_database_key_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keyed.db");
    SqliteRepo::open_with_key(&path, Some("test-key")).unwrap();

    let error = match SqliteRepo::open_with_key(&path, Some("wrong-key")) {
        Err(error) => error,
        Ok(_) => panic!("opening with a wrong key must fail"),
    };
    assert!(matches!(error, ilearned::AppError::DatabaseKey(_)));
    assert!(!error.to_string().contains("test-key"));
    assert!(!error.to_string().contains("wrong-key"));
}

#[test]
fn plaintext_database_rejects_keyed_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plaintext.db");
    SqliteRepo::open(&path).unwrap();

    let error = match SqliteRepo::open_with_key(&path, Some("test-key")) {
        Err(error) => error,
        Ok(_) => panic!("opening plaintext with a key must fail"),
    };
    assert!(matches!(error, ilearned::AppError::DatabaseKey(_)));
    assert!(error.to_string().contains("run ilearned db encrypt"));
}

#[test]
fn compound_key_unique() {
    let (_d, repo) = open_repo();
    repo.insert(&exp("rust", "a1", 0, State::Active)).unwrap();
    // Same (topic,id) twice => storage error; same id under another topic is fine.
    assert!(matches!(
        repo.insert(&exp("rust", "a1", 0, State::Active)),
        Err(ilearned::AppError::Storage(_))
    ));
    repo.insert(&exp("other", "a1", 0, State::Active)).unwrap();
}

#[test]
fn fts_bm25_and_topic_filter() {
    let (_d, repo) = open_repo();
    // "alpha" matches two records; one has two hits so BM25 ranks it first.
    let mut e1 = exp("rust", "a1", 0, State::Active);
    e1.when_text = "alpha alpha alpha".to_string();
    let mut e2 = exp("rust", "a2", 0, State::Active);
    e2.when_text = "alpha once".to_string();
    let e3 = exp("go", "b1", 0, State::Active);
    repo.insert(&e1).unwrap();
    repo.insert(&e2).unwrap();
    repo.insert(&e3).unwrap();

    let hits = repo.search_fts("alpha", None, false).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].0.id, "a1");

    let scoped = repo.search_fts("alpha", Some("rust"), false).unwrap();
    assert_eq!(scoped.len(), 2);

    // deep=false hides inactive; deep=true shows them.
    let mut old = exp("rust", "old", 0, State::Inactive);
    old.when_text = "alpha stale".to_string();
    repo.insert(&old).unwrap();
    assert_eq!(repo.search_fts("alpha", None, false).unwrap().len(), 3);
    assert_eq!(repo.search_fts("alpha", None, true).unwrap().len(), 4);

    // Deleted/forgotten never surface, even deep.
    let mut del = exp("rust", "del", 0, State::Deleted);
    del.when_text = "alpha gone".to_string();
    repo.insert(&del).unwrap();
    assert_eq!(repo.search_fts("alpha", None, true).unwrap().len(), 4);
}

#[test]
fn fts_invalid_syntax_is_typed() {
    let (_d, repo) = open_repo();
    repo.insert(&exp("rust", "a1", 0, State::Active)).unwrap();
    assert!(matches!(
        repo.search_fts("NEAR(broken", None, false),
        Err(ilearned::AppError::InvalidFtsSyntax(_))
    ));
}

#[test]
fn distinct_topics_respects_visibility() {
    let (_d, repo) = open_repo();
    repo.insert(&exp("b-active", "a1", 0, State::Active))
        .unwrap();
    repo.insert(&exp("a-inactive", "i1", 0, State::Inactive))
        .unwrap();
    repo.insert(&exp("c-deleted", "d1", 0, State::Deleted))
        .unwrap();
    repo.insert(&exp("d-forgotten", "f1", 0, State::Forgotten))
        .unwrap();
    assert_eq!(repo.distinct_topics(false).unwrap(), vec!["b-active"]);
    assert_eq!(
        repo.distinct_topics(true).unwrap(),
        vec!["a-inactive", "b-active"]
    );
}

#[test]
fn lifecycle_reconcile_and_purge() {
    use ilearned::domain::lifecycle::LifecycleConfig;
    use ilearned::storage::lifecycle::{purge_expired, reconcile_before_op};

    let (_d, repo) = open_repo();
    let cfg = LifecycleConfig::default();
    let now = Utc::now();

    // 61d-old active => inactive; boundary exact 60d stays active (strict >).
    repo.insert(&exp("t", "old61", 61, State::Active)).unwrap();
    let mut exact = exp("t", "exact60", 0, State::Active);
    exact.updated_at = now - Duration::days(60);
    repo.insert(&exact).unwrap();
    // 121d-old => forgotten with retention start.
    repo.insert(&exp("t", "old121", 121, State::Active))
        .unwrap();

    reconcile_before_op(&repo.conn(), now, &cfg).unwrap();
    assert_eq!(
        repo.get("t", "old61").unwrap().unwrap().state,
        State::Inactive
    );
    assert_eq!(
        repo.get("t", "exact60").unwrap().unwrap().state,
        State::Active
    );
    assert_eq!(
        repo.get("t", "old121").unwrap().unwrap().state,
        State::Forgotten
    );

    // Forgotten with 61d-old retention start purges with its FTS rows.
    let mut forg = repo.get("t", "old121").unwrap().unwrap();
    forg.updated_at = now - Duration::days(61);
    repo.update(&forg).unwrap();
    // update() clears retention metadata per contract; set it back manually
    // to simulate a retention start 61 days ago.
    repo.conn()
        .execute(
            "UPDATE experiences SET retention_started_at = ?1 WHERE topic='t' AND id='old121'",
            [(now - Duration::days(61)).timestamp()],
        )
        .unwrap();
    let purged = purge_expired(&repo.conn(), now, &cfg).unwrap();
    assert_eq!(purged, 1);
    assert!(repo.get("t", "old121").unwrap().is_none());
    assert!(
        repo.search_fts("trigger", None, true).unwrap().is_empty()
            || !repo
                .search_fts("trigger", None, true)
                .unwrap()
                .iter()
                .any(|(e, _)| e.id == "old121")
    );
}

#[test]
fn embedding_storage_uses_model_and_dimension_identity() {
    let (_d, repo) = open_repo();
    repo.upsert_vector("t", "id", "model", &[1.0, 0.0]).unwrap();
    repo.upsert_vector("t", "id", "model", &[1.0, 0.0, 0.0])
        .unwrap();

    assert_eq!(repo.load_vectors(None, "model", 2).unwrap().len(), 1);
    assert_eq!(repo.load_vectors(None, "model", 3).unwrap().len(), 1);
    assert!(repo.load_vectors(None, "model", 4).unwrap().is_empty());
}

#[test]
fn old_embedding_table_is_upgraded_without_losing_vectors() {
    let (_d, repo) = open_repo();
    repo.conn()
        .execute_batch(
            "CREATE TABLE embeddings (
                topic TEXT NOT NULL,
                id TEXT NOT NULL,
                model TEXT NOT NULL,
                dims INTEGER NOT NULL,
                vec BLOB NOT NULL,
                PRIMARY KEY (topic, id, model)
            );",
        )
        .unwrap();
    repo.conn()
        .execute(
            "INSERT INTO embeddings (topic,id,model,dims,vec) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![
                "t",
                "id",
                "model",
                2_i64,
                vec![0_u8, 0, 128, 63, 0, 0, 0, 0]
            ],
        )
        .unwrap();

    repo.upsert_vector("t", "id", "model", &[1.0, 0.0, 0.0])
        .unwrap();

    assert_eq!(repo.load_vectors(None, "model", 2).unwrap().len(), 1);
    assert_eq!(repo.load_vectors(None, "model", 3).unwrap().len(), 1);
}

#[test]
fn prune_vectors_preserves_target_identity() {
    let (_d, repo) = open_repo();
    repo.upsert_vector("t", "id", "target", &[1.0, 0.0])
        .unwrap();
    repo.upsert_vector("t", "id", "obsolete", &[1.0, 0.0])
        .unwrap();
    repo.upsert_vector("t", "id", "target", &[1.0, 0.0, 0.0])
        .unwrap();

    assert_eq!(repo.prune_vectors("target", 2).unwrap(), 2);
    assert_eq!(repo.load_vectors(None, "target", 2).unwrap().len(), 1);
    assert!(repo.load_vectors(None, "target", 3).unwrap().is_empty());
    assert!(repo.load_vectors(None, "obsolete", 2).unwrap().is_empty());
}

#[test]
fn embedding_candidates_include_non_deleted_states() {
    let (_d, repo) = open_repo();
    for (id, state) in [
        ("active", State::Active),
        ("inactive", State::Inactive),
        ("forgotten", State::Forgotten),
        ("deleted", State::Deleted),
    ] {
        repo.insert(&exp("migration", id, 0, state)).unwrap();
    }

    let candidates = repo.list_embedding_candidates().unwrap();
    assert_eq!(
        candidates.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["active", "forgotten", "inactive"]
    );
}

#[test]
fn encrypt_database_preserves_records_fts_and_embeddings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plaintext.db");
    let repo = SqliteRepo::open(&path).unwrap();
    repo.insert(&exp("migration", "record1", 0, State::Active))
        .unwrap();
    repo.upsert_vector("migration", "record1", "test-model", &[1.0, 0.0])
        .unwrap();
    repo.conn()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    drop(repo);

    encrypt_database(&path, "encryption-key").unwrap();

    let encrypted_bytes = std::fs::read(&path).unwrap();
    assert_ne!(&encrypted_bytes[..16], b"SQLite format 3\0");
    assert!(!path.with_extension("db-wal").exists());
    assert!(!path.with_extension("db-shm").exists());

    let reopened = SqliteRepo::open_with_key(&path, Some("encryption-key")).unwrap();
    assert!(reopened.get("migration", "record1").unwrap().is_some());
    assert_eq!(reopened.search_fts("alpha", None, false).unwrap().len(), 1);
    assert_eq!(
        reopened.load_vectors(None, "test-model", 2).unwrap().len(),
        1
    );
}

#[test]
fn encrypt_database_refuses_non_plaintext_sources() {
    let dir = tempfile::tempdir().unwrap();
    let encrypted_path = dir.path().join("encrypted.db");
    let encrypted_repo = SqliteRepo::open_with_key(&encrypted_path, Some("original-key")).unwrap();
    encrypted_repo
        .insert(&exp("migration", "record1", 0, State::Active))
        .unwrap();
    drop(encrypted_repo);
    let encrypted_before = std::fs::read(&encrypted_path).unwrap();

    assert!(encrypt_database(&encrypted_path, "new-key").is_err());
    assert_eq!(std::fs::read(&encrypted_path).unwrap(), encrypted_before);

    let invalid_path = dir.path().join("not-sqlite.db");
    std::fs::write(&invalid_path, b"not a SQLite database").unwrap();
    let invalid_before = std::fs::read(&invalid_path).unwrap();

    assert!(encrypt_database(&invalid_path, "encryption-key").is_err());
    assert_eq!(std::fs::read(&invalid_path).unwrap(), invalid_before);
}

#[test]
fn encrypt_database_requires_existing_nonempty_key() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("plaintext.db");
    SqliteRepo::open(&source).unwrap();

    assert!(encrypt_database(&source, "").is_err());
    assert!(source.exists());

    let missing = dir.path().join("missing.db");
    assert!(encrypt_database(&missing, "encryption-key").is_err());
    assert!(!missing.exists());
}

#[test]
fn encrypt_database_refuses_while_a_repository_holds_the_database_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plaintext.db");
    let repo = SqliteRepo::open(&path).unwrap();
    repo.insert(&exp("migration", "record1", 0, State::Active))
        .unwrap();

    let error = encrypt_database(&path, "encryption-key").unwrap_err();

    assert!(error.to_string().contains("in use"));
    assert_eq!(
        repo.get("migration", "record1").unwrap().unwrap().id,
        "record1"
    );
    assert_eq!(&std::fs::read(&path).unwrap()[..16], b"SQLite format 3\0");
}

#[test]
fn encrypt_database_failure_cleans_temporary_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incomplete.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE unrelated (id INTEGER PRIMARY KEY);")
        .unwrap();
    let before = std::fs::read(&path).unwrap();

    assert!(encrypt_database(&path, "encryption-key").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ilearned-encrypt-")
    }));
}
