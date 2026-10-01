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
