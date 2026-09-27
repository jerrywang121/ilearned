use chrono::Utc;
use ilearned::application::MemoryService;
use ilearned::domain::lifecycle::LifecycleConfig;
use ilearned::domain::{AddCommand, FeedbackCommand, ModifyCommand, SearchQuery, State};
use ilearned::storage::repository::ExperienceRepo;
use ilearned::storage::SqliteRepo;

fn svc() -> (tempfile::TempDir, MemoryService<SqliteRepo>) {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepo::open(&dir.path().join("t.db")).unwrap();
    let s = MemoryService::new(repo, LifecycleConfig::default());
    (dir, s)
}

fn add_cmd(topic: &str) -> AddCommand {
    AddCommand {
        topic: topic.to_string(),
        when_text: "when deploy fails".to_string(),
        if_text: "alert fires".to_string(),
        do_text: "restart worker".to_string(),
        check_text: "health ok".to_string(),
    }
}

#[test]
fn add_modify_promote_delete_flow() {
    let (_d, s) = svc();
    let e = s.add(add_cmd("rust")).unwrap();
    assert_eq!(e.good_count, 1);
    assert_eq!(e.bad_count, 0);
    assert_eq!(e.state, State::Active);
    assert_eq!(e.id.len(), 8);

    // Promote bumps good_count.
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    let p = s.promote(&f).unwrap();
    assert_eq!(p.good_count, 2);

    // Force forgotten by backdating, then reconcile via next op; modify
    // restores forgotten -> active.
    {
        let repo = s.repo();
        let mut stored = repo.get(&e.topic, &e.id).unwrap().unwrap();
        stored.updated_at = Utc::now() - chrono::Duration::days(130);
        stored.state = State::Forgotten;
        repo.update(&stored).unwrap();
        repo.conn()
            .execute(
                "UPDATE experiences SET retention_started_at = ?1 WHERE topic=?2 AND id=?3",
                rusqlite::params![
                    (Utc::now() - chrono::Duration::days(1)).timestamp(),
                    e.topic,
                    e.id
                ],
            )
            .unwrap();
    }
    let m = s
        .modify(ModifyCommand {
            topic: e.topic.clone(),
            id: e.id.clone(),
            when_text: Some("new when".to_string()),
            if_text: None,
            do_text: None,
            check_text: None,
        })
        .unwrap();
    assert_eq!(m.state, State::Active);
    assert_eq!(m.when_text, "new when");

    // Delete then modify => NotFound; second delete idempotent.
    s.delete(&e.topic, &e.id).unwrap();
    assert!(matches!(
        s.modify(ModifyCommand {
            topic: e.topic.clone(),
            id: e.id.clone(),
            when_text: Some("x".to_string()),
            ..Default::default()
        }),
        Err(ilearned::AppError::NotFound { .. })
    ));
    s.delete(&e.topic, &e.id).unwrap();
}

#[test]
fn search_visibility_and_pagination() {
    let (_d, s) = svc();
    // Seed via service, then flip states directly in storage.
    let a = s.add(add_cmd("t")).unwrap();
    let b = s.add(add_cmd("t")).unwrap();
    let c = s.add(add_cmd("t")).unwrap();
    {
        let repo = s.repo();
        for (id, st) in [
            (a.id.clone(), State::Inactive),
            (b.id.clone(), State::Deleted),
            (c.id.clone(), State::Forgotten),
        ] {
            let mut e = repo.get("t", &id).unwrap().unwrap();
            e.state = st;
            repo.update(&e).unwrap();
        }
    }
    let base = SearchQuery {
        topic: Some("t".to_string()),
        ..Default::default()
    };
    // Shallow: only active.
    let shallow = s.search(&base).unwrap();
    assert!(shallow.iter().all(|e| e.state == State::Active));
    // Deep: active + inactive, still no deleted/forgotten.
    let deep = s
        .search(&SearchQuery {
            deep: true,
            ..base.clone()
        })
        .unwrap();
    assert!(deep.iter().any(|e| e.state == State::Inactive));
    assert!(deep
        .iter()
        .all(|e| { e.state == State::Active || e.state == State::Inactive }));

    // Pagination: limit=0 => []; offset beyond => []; limit>100 clamps (no error).
    assert!(s
        .search(&SearchQuery {
            limit: 0,
            ..base.clone()
        })
        .unwrap()
        .is_empty());
    assert!(s
        .search(&SearchQuery {
            offset: 9999,
            ..base.clone()
        })
        .unwrap()
        .is_empty());
    let _ = s
        .search(&SearchQuery {
            limit: 10_000,
            ..base.clone()
        })
        .unwrap();
}

#[test]
fn semantic_without_provider_is_typed() {
    let (_d, s) = svc();
    s.add(add_cmd("t")).unwrap();
    assert!(matches!(
        s.search(&SearchQuery {
            semantic: Some("hello".to_string()),
            ..Default::default()
        }),
        Err(ilearned::AppError::EmbeddingUnavailable(_))
    ));
}
