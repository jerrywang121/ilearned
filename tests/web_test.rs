use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

struct TestServer {
    _dir: tempfile::TempDir,
    port: u16,
    _child: tokio::process::Child,
}

impl TestServer {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

async fn spawn_server() -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"));
    let db = dir.path().join("t.db").to_string_lossy().to_string();
    let bind = format!("127.0.0.1:{port}");
    let child = tokio::process::Command::new(bin)
        .args(["--db", &db, "serve", "--bind", &bind])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let srv = TestServer {
        _dir: dir,
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
