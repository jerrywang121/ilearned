use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use ilearned::storage::repository::ExperienceRepo;

struct TestServer {
    dir: tempfile::TempDir,
    port: u16,
    _child: tokio::process::Child,
}

fn legacy_exp(topic: &str, id: &str) -> ilearned::domain::Experience {
    ilearned::domain::Experience {
        topic: topic.to_string(),
        id: id.to_string(),
        when_text: "w".to_string(),
        if_text: "i".to_string(),
        do_text: "d".to_string(),
        check_text: "c".to_string(),
        updated_at: chrono::Utc::now(),
        good_count: 1,
        bad_count: 0,
        state: ilearned::domain::State::Active,
    }
}

impl TestServer {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn db_path(&self) -> std::path::PathBuf {
        self.dir.path().join("t.db")
    }
}

async fn spawn_server() -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("db = {:?}\n", db.to_string_lossy())).unwrap();
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"));
    let bind = format!("127.0.0.1:{port}");
    let child = tokio::process::Command::new(bin)
        .args([
            "--config-file",
            &config.to_string_lossy(),
            "serve",
            "--bind",
            &bind,
        ])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let srv = TestServer {
        dir,
        port,
        _child: child,
    };
    let client = reqwest::Client::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("server did not become ready");
        }
        if client
            .get(srv.url("/healthz"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return srv;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn pages_render_and_escape() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();

    // Seed via REST (canonical path), with an XSS payload in a field.
    let created: serde_json::Value = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({"topic":"web","when":"<script>alert(1)</script>","if":"i","do":"d","check":"c"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let (topic, id) = (
        created["topic"].as_str().unwrap(),
        created["id"].as_str().unwrap(),
    );

    // Index shows the escaped payload, not the raw tag.
    let index = client
        .get(srv.url("/?topic=web"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(index.contains("&lt;script&gt;"), "index must escape HTML");
    assert!(!index.contains("<script>alert(1)</script>"));

    // Detail page renders the record.
    let detail = client
        .get(srv.url(&format!("/experiences/{topic}/{id}")))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(detail.contains(topic));
    assert!(detail.contains(id));
    assert!(!detail.contains("<script>alert(1)</script>"));
}

#[tokio::test]
async fn nested_topic_add_redirect_and_link_open_detail() {
    let srv = spawn_server().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let response = client
        .post(srv.url("/experiences"))
        .form(&[
            ("topic", "jump/down/up"),
            ("when_text", "w"),
            ("if_text", "i"),
            ("do_text", "d"),
            ("check", "c"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.starts_with("/experiences/jump%2Fdown%2Fup/"));

    let detail = client.get(srv.url(&location)).send().await.unwrap();
    assert_eq!(detail.status(), reqwest::StatusCode::OK);

    let index = client.get(srv.url("/")).send().await.unwrap();
    let index = index.text().await.unwrap();
    assert!(index.contains(&format!("href=\"{location}\"")));
}

#[tokio::test]
async fn topics_page_lists_and_escapes() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({"topic":"travel/hotel","when":"w","if":"i","do":"d","check":"c"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    // Index links to /topics.
    let index = client
        .get(srv.url("/"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(index.contains("/topics"), "index must link to topics page");
    // /topics lists the topic; ?q= filters.
    let page = client
        .get(srv.url("/topics"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("travel/hotel"));
    let filtered = client
        .get(srv.url("/topics?q=travel"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(filtered.contains("travel/hotel"));
    assert!(!page.contains("<script>"));
}

#[tokio::test]
async fn topics_page_escapes_legacy() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let db_path = srv.db_path();
    // Legacy row bypassing service validation (pre-hierarchy data).
    {
        let repo = ilearned::storage::SqliteRepo::open(std::path::Path::new(&db_path)).unwrap();
        repo.insert(&legacy_exp("<b>x</b>/y", "legacy01")).unwrap();
    }
    let page = client
        .get(srv.url("/topics"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("&lt;b&gt;x&lt;/b&gt;/y"),
        "legacy topic must render escaped: {page}"
    );
    assert!(
        !page.contains("<b>x</b>"),
        "raw legacy markup must not leak"
    );
}

#[tokio::test]
async fn forms_validate_and_confirm() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();

    // POST add with missing check => 400.
    let r = client
        .post(srv.url("/experiences"))
        .form(&[("topic", "t"), ("when", "w"), ("if", "i"), ("do", "d")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);

    // Seed one record, then POST delete without confirm=yes => 400 and record remains.
    let created: serde_json::Value = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({"topic":"t","when":"w","if":"i","do":"d","check":"c"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let (topic, id) = (
        created["topic"].as_str().unwrap().to_string(),
        created["id"].as_str().unwrap().to_string(),
    );
    let r = client
        .post(srv.url(&format!("/experiences/{topic}/{id}/delete")))
        .form(&[("ack", "no")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let still: serde_json::Value = client
        .get(srv.url("/api/v1/experiences?topic=t"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(still.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn index_renders_topic_sorted_table_with_operations() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let mut ids = Vec::new();

    for topic in ["alpha", "beta", "zeta"] {
        if topic == "zeta" {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let created: serde_json::Value = client
            .post(srv.url("/api/v1/experiences"))
            .json(&serde_json::json!({
                "topic": topic,
                "when": format!("when {}", topic),
                "if": format!("if {}", topic),
                "do": format!("do {}", topic),
                "check": format!("check {}", topic)
            }))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        ids.push((topic.to_owned(), created["id"].as_str().unwrap().to_owned()));
    }

    let index = client
        .get(srv.url("/"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();

    for column in ["Topic", "When", "If", "Do", "Check", "Ops"] {
        assert!(
            index.contains(&format!("<th>{}</th>", column)),
            "missing {} column",
            column
        );
    }
    assert!(
        index.contains(r#"#search input[name="q"],#search input[name="topic"]{width:40ch"#),
        "search and topic filters should be wider"
    );
    assert!(index.contains(r#"<div class="table-wrap"><table>"#));
    assert!(index.contains(".table-wrap{overflow-x:auto}"));
    assert!(index.contains("min-width:50em"));

    let alpha = index.find("alpha").expect("alpha topic is listed");
    let beta = index.find("beta").expect("beta topic is listed");
    let zeta = index.find("zeta").expect("zeta topic is listed");
    assert!(
        alpha < beta && beta < zeta,
        "rows should be ordered by topic"
    );

    for symbol in ["✎", "★", "☆", "✘"] {
        assert!(index.contains(symbol), "missing {} operation", symbol);
    }
    assert!(index.contains(r#"class="delete-op""#));
    assert!(
        index.contains("color:red"),
        "delete operation should be red"
    );

    let alpha_id = ids
        .iter()
        .find(|(topic, _)| topic.as_str() == "alpha")
        .map(|(_, id)| id)
        .unwrap();
    for label in ["Edit", "Promote", "Demote", "Delete"] {
        assert!(index.contains(&format!(r#"aria-label="{} alpha/{}""#, label, alpha_id)));
    }
    assert!(index.contains(&format!(r#"href="/experiences/alpha/{}/edit""#, alpha_id)));
    for operation in ["promote", "demote", "delete"] {
        assert!(index.contains(&format!(
            r#"action="/experiences/alpha/{}/{}""#,
            alpha_id, operation
        )));
    }
}

#[tokio::test]
async fn edit_form_renders_wide_multiline_fields() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({
            "topic": "forms",
            "when": "arrival scenario",
            "if": "trigger observed",
            "do": "take action",
            "check": "verify result"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();

    let edit = client
        .get(srv.url(&format!("/experiences/forms/{}/edit", id)))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();

    for (name, value) in [
        ("when_text", "arrival scenario"),
        ("if_text", "trigger observed"),
        ("do_text", "take action"),
        ("check", "verify result"),
    ] {
        assert!(edit.contains(&format!(
            r#"<textarea name="{}">{}</textarea>"#,
            name, value
        )));
    }
    assert!(
        edit.contains("max-width:96vw"),
        "edit page should use a wide layout"
    );
    assert!(edit.contains("textarea{display:block;width:100%"));
}

#[tokio::test]
async fn index_row_actions_encode_nested_topics() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({
            "topic": "nested/child",
            "when": "nested scenario",
            "if": "trigger observed",
            "do": "take action",
            "check": "verify result"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();

    let index = client
        .get(srv.url("/"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();

    for path in [
        format!(r#"href="/experiences/nested%2Fchild/{}/edit""#, id),
        format!(r#"action="/experiences/nested%2Fchild/{}/promote""#, id),
        format!(r#"action="/experiences/nested%2Fchild/{}/demote""#, id),
        format!(r#"action="/experiences/nested%2Fchild/{}/delete""#, id),
    ] {
        assert!(
            index.contains(&path),
            "missing encoded action path: {}",
            path
        );
    }
}
