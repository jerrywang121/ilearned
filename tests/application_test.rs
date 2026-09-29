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

#[test]
fn semantic_ranks_by_cosine() {
    use ilearned::embedding::FakeEmbeddingProvider;
    let (_d, s) = svc();
    let s = s.with_embedding_provider(FakeEmbeddingProvider::new());
    // Fake embeds by token hash; identical text => cosine 1.0 on top.
    let target = s.add(add_cmd("t")).unwrap();
    s.add(AddCommand {
        topic: "t".to_string(),
        when_text: "completely different words here".to_string(),
        if_text: "nothing shared xyz".to_string(),
        do_text: "other action qqq".to_string(),
        check_text: "other signal www".to_string(),
    })
    .unwrap();
    let hits = s
        .search(&SearchQuery {
            semantic: Some("when deploy fails alert fires restart worker health ok".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, target.id);
}

#[test]
fn rrf_is_deterministic() {
    use ilearned::embedding::FakeEmbeddingProvider;
    let (_d, s) = svc();
    let s = s.with_embedding_provider(FakeEmbeddingProvider::new());
    s.add(add_cmd("t")).unwrap();
    s.add(add_cmd("t")).unwrap();
    let q = SearchQuery {
        text: Some("deploy".to_string()),
        semantic: Some("deploy worker".to_string()),
        ..Default::default()
    };
    let first = s.search(&q).unwrap();
    let second = s.search(&q).unwrap();
    assert_eq!(
        first.iter().map(|e| &e.id).collect::<Vec<_>>(),
        second.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

#[test]
fn combined_search_embed_failure_is_typed() {
    use ilearned::embedding::FailingEmbeddingProvider;
    let (_d, s) = svc();
    let s = s.with_embedding_provider(FailingEmbeddingProvider);
    s.add(add_cmd("t")).unwrap();
    // add() itself must succeed (best-effort); search must fail typed.
    assert!(matches!(
        s.search(&SearchQuery {
            text: Some("deploy".to_owned()),
            semantic: Some("deploy".to_owned()),
            ..Default::default()
        }),
        Err(ilearned::AppError::EmbeddingUnavailable(_))
    ));
}

#[test]
fn add_succeeds_when_embed_fails() {
    use ilearned::embedding::FailingEmbeddingProvider;
    let (_d, s) = svc();
    let s = s.with_embedding_provider(FailingEmbeddingProvider);
    let e = s.add(add_cmd("t")).unwrap();
    assert_eq!(e.state, State::Active);
    let got = s
        .search(&SearchQuery {
            topic: Some("t".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert!(got.iter().any(|x| x.id == e.id));
}

#[test]
fn delete_missing_returns_not_found() {
    let (_d, s) = svc();
    assert!(matches!(
        s.delete("no", "such"),
        Err(ilearned::AppError::NotFound { .. })
    ));
}

#[test]
fn get_hides_deleted_but_shows_forgotten_and_inactive() {
    let (_d, s) = svc();
    let e = s.add(add_cmd("t")).unwrap();
    assert!(s.get(&e.topic, &e.id).is_ok());
    {
        let repo = s.repo();
        let mut stored = repo.get(&e.topic, &e.id).unwrap().unwrap();
        stored.state = ilearned::domain::State::Forgotten;
        repo.update(&stored).unwrap();
    }
    assert!(s.get(&e.topic, &e.id).is_ok());
    s.delete(&e.topic, &e.id).unwrap();
    assert!(matches!(
        s.get(&e.topic, &e.id),
        Err(ilearned::AppError::NotFound { .. })
    ));
}

#[test]
fn add_rejects_invalid_topic() {
    let (_d, s) = svc();
    let mut cmd = add_cmd("Travel/Hotel");
    cmd.topic = "Travel/Hotel".to_string();
    assert!(matches!(
        s.add(cmd),
        Err(ilearned::AppError::InvalidInput(_))
    ));
}

#[test]
fn search_wildcard_middle_hash() {
    let (_d, s) = svc();
    s.add(add_cmd("travel/hotel/checkout")).unwrap();
    s.add(add_cmd("travel/flight/checkout")).unwrap();
    s.add(add_cmd("other/x")).unwrap();
    let hits = s
        .search(&SearchQuery {
            topic: Some("travel/#/checkout".to_string()),
            ..Default::default()
        })
        .unwrap();
    let topics: Vec<&str> = hits.iter().map(|e| e.topic.as_str()).collect();
    assert_eq!(topics.len(), 2);
    assert!(topics.contains(&"travel/hotel/checkout"));
    assert!(topics.contains(&"travel/flight/checkout"));
}

#[test]
fn search_bare_topic_is_exact_only() {
    let (_d, s) = svc();
    s.add(add_cmd("travel")).unwrap();
    s.add(add_cmd("travel/hotel")).unwrap();
    let hits = s
        .search(&SearchQuery {
            topic: Some("travel".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].topic, "travel");
}

#[test]
fn list_topics_level_query_pagination() {
    use ilearned::domain::TopicQuery;
    let (_d, s) = svc();
    s.add(add_cmd("travel/hotel/checkout")).unwrap();
    s.add(add_cmd("travel/hotel/lobby")).unwrap();
    s.add(add_cmd("other/x")).unwrap();
    // level dedup: first two segments only.
    let got = s
        .list_topics(&TopicQuery {
            level: Some(2),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(got, vec!["other/x", "travel/hotel"]);
    // substring query matches full topic, truncation applies after.
    let got = s
        .list_topics(&TopicQuery {
            query: Some("hot".to_string()),
            level: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(got, vec!["travel"]);
    // query is lowercased before matching: uppercase substring and
    // uppercase `#` pattern both match lowercase topics.
    let got = s
        .list_topics(&TopicQuery {
            query: Some("HOT".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(got, vec!["travel/hotel/checkout", "travel/hotel/lobby"]);
    let got = s
        .list_topics(&TopicQuery {
            query: Some("Travel/#".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(got, vec!["travel/hotel/checkout", "travel/hotel/lobby"]);
    // pagination clamps; level=0 is invalid.
    assert!(s
        .list_topics(&TopicQuery {
            limit: 0,
            ..Default::default()
        })
        .unwrap()
        .is_empty());
    assert!(matches!(
        s.list_topics(&TopicQuery {
            level: Some(0),
            ..Default::default()
        }),
        Err(ilearned::AppError::InvalidInput(_))
    ));
}

#[test]
fn clear_rejects_wildcard_topic() {
    use ilearned::domain::ClearCommand;
    let (_d, s) = svc();
    s.add(add_cmd("travel/hotel")).unwrap();
    assert!(matches!(
        s.clear(&ClearCommand::Topic("travel/#".to_string())),
        Err(ilearned::AppError::InvalidInput(_))
    ));
}

#[test]
fn semantic_search_respects_middle_hash() {
    use ilearned::embedding::FakeEmbeddingProvider;
    let (_d, s) = svc();
    let s = s.with_embedding_provider(FakeEmbeddingProvider::new());
    s.add(add_cmd("travel/hotel/checkout")).unwrap();
    s.add(add_cmd("other/x")).unwrap();
    let hits = s
        .search(&SearchQuery {
            topic: Some("travel/#/checkout".to_string()),
            semantic: Some("deploy worker".to_string()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].topic, "travel/hotel/checkout");
}
