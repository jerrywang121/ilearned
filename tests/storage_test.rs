use chrono::{Duration, Utc};
use ilearned::domain::{Experience, State};
use ilearned::storage::repository::ExperienceRepo;
use ilearned::storage::SqliteRepo;

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
    repo.insert(&exp("b-active", "a1", 0, State::Active)).unwrap();
    repo.insert(&exp("a-inactive", "i1", 0, State::Inactive))
        .unwrap();
    repo.insert(&exp("c-deleted", "d1", 0, State::Deleted)).unwrap();
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
