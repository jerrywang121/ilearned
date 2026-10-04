use async_trait::async_trait;
use chrono::Utc;
use ilearned::application::MemoryService;
use ilearned::domain::lifecycle::LifecycleConfig;
use ilearned::domain::{
    AddCommand, Experience, FeedbackCommand, SearchQuery, State, UpdateCommand,
};
use ilearned::embedding::EmbeddingProvider;
use ilearned::error::AppError;
use ilearned::storage::embeddings::VectorStore;
use ilearned::storage::repository::ExperienceRepo;
use ilearned::storage::SqliteRepo;
use std::sync::{Arc, Mutex};

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

#[derive(Clone)]
struct TestEmbeddingProvider {
    model: String,
    dimensions: Option<usize>,
    vectors: Vec<Vec<f32>>,
    calls: Arc<Mutex<usize>>,
    fail_at: Option<usize>,
}

impl TestEmbeddingProvider {
    fn new(
        model: &str,
        dimensions: Option<usize>,
        vectors: Vec<Vec<f32>>,
        fail_at: Option<usize>,
    ) -> Self {
        Self {
            model: model.to_string(),
            dimensions,
            vectors,
            calls: Arc::new(Mutex::new(0)),
            fail_at,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for TestEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, AppError> {
        let index = {
            let mut calls = self.calls.lock().expect("call counter lock");
            let index = *calls;
            *calls += 1;
            index
        };
        if self.fail_at == Some(index) {
            return Err(AppError::EmbeddingUnavailable(
                "test provider failure".to_string(),
            ));
        }
        Ok(self
            .vectors
            .get(index)
            .cloned()
            .or_else(|| self.vectors.last().cloned())
            .unwrap_or_default())
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> Option<usize> {
        self.dimensions
    }
}

fn migration_exp(id: &str, state: State) -> Experience {
    Experience {
        topic: "migration".to_string(),
        id: id.to_string(),
        when_text: "when migration runs".to_string(),
        if_text: "if the provider responds".to_string(),
        do_text: "stage the vector".to_string(),
        check_text: "the target row exists".to_string(),
        updated_at: Utc::now(),
        good_count: 1,
        bad_count: 0,
        state,
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

    // Force forgotten by backdating, then reconcile via next op; update
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
        .update(UpdateCommand {
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

    // Delete then update => NotFound; second delete idempotent.
    s.delete(&e.topic, &e.id).unwrap();
    assert!(matches!(
        s.update(UpdateCommand {
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

fn svc_with_threshold(threshold: f64) -> (tempfile::TempDir, MemoryService<SqliteRepo>) {
    let dir = tempfile::tempdir().unwrap();
    let repo = SqliteRepo::open(&dir.path().join("t.db")).unwrap();
    let s = MemoryService::new(
        repo,
        LifecycleConfig {
            auto_delete_threshold: threshold,
            ..LifecycleConfig::default()
        },
    );
    (dir, s)
}

#[test]
fn demote_below_threshold_auto_deletes() {
    let (_d, s) = svc_with_threshold(0.2);
    // add sets good=1, bad=0. Two demotes: score 1/2=0.5 (survives), then
    // 1/3≈0.333 (survives), third demote: 1/4=0.25 (survives), fourth:
    // 1/5=0.2 == threshold (survives, strict <), fifth: 1/6≈0.167 < 0.2
    // (auto-deleted).
    let e = s.add(add_cmd("rust")).unwrap();
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    for _ in 0..4 {
        let d = s.demote(&f).unwrap();
        assert_eq!(d.state, State::Active);
    }
    assert_eq!(s.demote(&f).unwrap().state, State::Deleted);
    // Auto-deleted reads as NotFound for both get and demote (demote on a
    // deleted record is NotFound, matching existing feedback semantics).
    assert!(matches!(
        s.get(&e.topic, &e.id),
        Err(ilearned::AppError::NotFound { .. })
    ));
    assert!(matches!(
        s.demote(&f),
        Err(ilearned::AppError::NotFound { .. })
    ));
}

#[test]
fn demote_score_equal_threshold_survives() {
    let (_d, s) = svc_with_threshold(0.5);
    // good=1: after one demote score = 1/2 = 0.5 == threshold -> survives.
    let e = s.add(add_cmd("rust")).unwrap();
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    let d = s.demote(&f).unwrap();
    assert_eq!(d.state, State::Active);
    assert_eq!(d.bad_count, 1);
}

#[test]
fn demote_default_threshold_0_3() {
    let (_d, s) = svc();
    // Default 0.3: good=1, scores 1/2=0.5 and 1/3≈0.333 survive; 1/4=0.25
    // < 0.3 auto-deletes on the third demote.
    let e = s.add(add_cmd("rust")).unwrap();
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    for _ in 0..2 {
        let d = s.demote(&f).unwrap();
        assert_eq!(d.state, State::Active);
    }
    let d = s.demote(&f).unwrap();
    assert_eq!(d.state, State::Deleted);
    assert_eq!(d.bad_count, 3);
}

#[test]
fn demote_threshold_zero_never_auto_deletes() {
    let (_d, s) = svc_with_threshold(0.0);
    let e = s.add(add_cmd("rust")).unwrap();
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    for _ in 0..10 {
        let d = s.demote(&f).unwrap();
        assert_eq!(d.state, State::Active);
    }
}

#[test]
fn demote_threshold_one_auto_deletes_on_first_demote() {
    let (_d, s) = svc_with_threshold(1.0);
    // good=1, bad=0 -> after demote score = 1/2 = 0.5 < 1.0 -> deleted.
    let e = s.add(add_cmd("rust")).unwrap();
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    let d = s.demote(&f).unwrap();
    assert_eq!(d.state, State::Deleted);
    assert_eq!(d.bad_count, 1);
}

#[test]
fn demote_auto_delete_starts_retention_clock() {
    let (_d, s) = svc_with_threshold(1.0);
    let e = s.add(add_cmd("rust")).unwrap();
    let f = FeedbackCommand {
        topic: e.topic.clone(),
        id: e.id.clone(),
    };
    s.demote(&f).unwrap();
    let repo = s.repo();
    let stored = repo.get(&e.topic, &e.id).unwrap().unwrap();
    assert_eq!(stored.state, State::Deleted);
    let retention = repo
        .conn()
        .query_row(
            "SELECT retention_started_at FROM experiences WHERE topic=?1 AND id=?2",
            rusqlite::params![e.topic, e.id],
            |row| row.get::<_, Option<i64>>("retention_started_at"),
        )
        .unwrap();
    assert!(
        retention.is_some(),
        "retention clock must start on auto-delete"
    );
}

#[test]
fn migration_reembeds_non_deleted_records_and_preserves_canonical_data() {
    let (_d, base) = svc();
    let s = base.with_embedding_provider(TestEmbeddingProvider::new(
        "new-model",
        Some(3),
        vec![vec![1.0, 0.0, 0.0]],
        None,
    ));
    let states = [
        ("active", State::Active),
        ("inactive", State::Inactive),
        ("forgotten", State::Forgotten),
        ("deleted", State::Deleted),
    ];
    for (id, state) in states {
        let e = migration_exp(id, state);
        s.repo().insert(&e).unwrap();
        s.repo()
            .upsert_vector(&e.topic, &e.id, "old-model", &[1.0, 0.0])
            .unwrap();
    }
    let before = states
        .iter()
        .map(|(id, _)| s.repo().get("migration", id).unwrap().unwrap())
        .collect::<Vec<_>>();

    let summary = s.migrate_embeddings(false).unwrap();

    assert_eq!(summary.model, "new-model");
    assert_eq!(summary.dims, Some(3));
    assert_eq!(summary.total, 3);
    assert_eq!(summary.migrated, 3);
    assert_eq!(summary.pruned, 0);
    let target = s.repo().load_vectors(None, "new-model", 3).unwrap();
    assert_eq!(target.len(), 3);
    assert!(target.iter().all(|(_, id, _)| id != "deleted"));
    assert_eq!(
        s.repo().load_vectors(None, "old-model", 2).unwrap().len(),
        4
    );
    let after = states
        .iter()
        .map(|(id, _)| s.repo().get("migration", id).unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(before, after);
}

#[test]
fn migration_failure_preserves_old_vectors_and_skips_prune() {
    let (_d, base) = svc();
    let s = base.with_embedding_provider(TestEmbeddingProvider::new(
        "new-model",
        Some(3),
        vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]],
        Some(1),
    ));
    for id in ["a", "b"] {
        let e = migration_exp(id, State::Active);
        s.repo().insert(&e).unwrap();
        s.repo()
            .upsert_vector(&e.topic, &e.id, "old-model", &[1.0, 0.0])
            .unwrap();
    }
    s.repo()
        .upsert_vector("migration", "a", "obsolete-model", &[0.0, 1.0])
        .unwrap();

    let err = s.migrate_embeddings(true).unwrap_err();

    assert!(matches!(err, AppError::EmbeddingUnavailable(_)));
    assert_eq!(
        s.repo().load_vectors(None, "old-model", 2).unwrap().len(),
        2
    );
    assert_eq!(
        s.repo().load_vectors(None, "new-model", 3).unwrap().len(),
        1
    );
    assert_eq!(
        s.repo()
            .load_vectors(None, "obsolete-model", 2)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn migration_rejects_dimension_mismatch() {
    let (_d, base) = svc();
    let s = base.with_embedding_provider(TestEmbeddingProvider::new(
        "new-model",
        Some(3),
        vec![vec![1.0, 0.0]],
        None,
    ));
    let e = migration_exp("mismatch", State::Active);
    s.repo().insert(&e).unwrap();

    let err = s.migrate_embeddings(false).unwrap_err();

    assert!(matches!(err, AppError::EmbeddingUnavailable(_)));
    assert!(s
        .repo()
        .load_vectors(None, "new-model", 2)
        .unwrap()
        .is_empty());
    assert!(s
        .repo()
        .load_vectors(None, "new-model", 3)
        .unwrap()
        .is_empty());
}

#[test]
fn migration_rejects_empty_vectors() {
    let (_d, base) = svc();
    let s = base.with_embedding_provider(TestEmbeddingProvider::new(
        "new-model",
        None,
        vec![vec![]],
        None,
    ));
    let e = migration_exp("empty", State::Active);
    s.repo().insert(&e).unwrap();

    let err = s.migrate_embeddings(false).unwrap_err();

    assert!(matches!(err, AppError::EmbeddingUnavailable(_)));
    assert!(s
        .repo()
        .load_vectors(None, "new-model", 0)
        .unwrap()
        .is_empty());
}

#[test]
fn migration_rejects_inconsistent_undeclared_dimensions() {
    let (_d, base) = svc();
    let s = base.with_embedding_provider(TestEmbeddingProvider::new(
        "new-model",
        None,
        vec![vec![1.0, 0.0], vec![1.0, 0.0, 0.0]],
        None,
    ));
    for id in ["a", "b"] {
        let e = migration_exp(id, State::Active);
        s.repo().insert(&e).unwrap();
        s.repo()
            .upsert_vector(&e.topic, &e.id, "old-model", &[1.0, 0.0])
            .unwrap();
    }
    s.repo()
        .upsert_vector("migration", "a", "obsolete-model", &[0.0, 1.0])
        .unwrap();

    let err = s.migrate_embeddings(true).unwrap_err();

    assert!(matches!(err, AppError::EmbeddingUnavailable(_)));
    assert_eq!(
        s.repo().load_vectors(None, "new-model", 2).unwrap().len(),
        1
    );
    assert!(s
        .repo()
        .load_vectors(None, "new-model", 3)
        .unwrap()
        .is_empty());
    assert_eq!(
        s.repo().load_vectors(None, "old-model", 2).unwrap().len(),
        2
    );
    assert_eq!(
        s.repo()
            .load_vectors(None, "obsolete-model", 2)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn semantic_search_does_not_use_mismatched_vectors() {
    let (_d, base) = svc();
    let s = base.with_embedding_provider(TestEmbeddingProvider::new(
        "test-model",
        Some(3),
        vec![vec![1.0, 0.0, 0.0]],
        None,
    ));
    let e = migration_exp("semantic", State::Active);
    s.repo().insert(&e).unwrap();
    s.repo()
        .upsert_vector(&e.topic, &e.id, "test-model", &[1.0, 0.0])
        .unwrap();

    let hits = s
        .search(&SearchQuery {
            semantic: Some("query".to_string()),
            ..Default::default()
        })
        .unwrap();

    assert!(hits.is_empty());
}

#[test]
fn migration_rejects_zero_declared_dimension_before_prune() {
    let (_dir, base) = svc();
    base.repo()
        .upsert_vector("old", "id", "old-model", &[1.0, 0.0])
        .unwrap();
    let service = base.with_embedding_provider(TestEmbeddingProvider::new(
        "zero-model",
        Some(0),
        vec![],
        None,
    ));

    let err = service.migrate_embeddings(true).unwrap_err();

    assert!(matches!(err, AppError::EmbeddingUnavailable(_)));
    assert_eq!(
        service
            .repo()
            .load_vectors(None, "old-model", 2)
            .unwrap()
            .len(),
        1
    );
}
