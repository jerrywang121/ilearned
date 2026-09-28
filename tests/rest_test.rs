use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde_json::Value;

fn base(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

struct TestServer {
    _dir: tempfile::TempDir,
    port: u16,
    _child: tokio::process::Child,
}

impl TestServer {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", base(self.port))
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
async fn healthz_and_crud() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();

    let h: Value = client
        .get(srv.url("/healthz"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["ok"], true);

    // POST add => 201.
    let created: Value = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({"topic":"rust","when":"w","if":"i","do":"d","check":"c"}))
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

    // GET search finds it.
    let list: Value = client
        .get(srv.url("/api/v1/experiences?topic=rust"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    // PATCH modify.
    let patched: Value = client
        .patch(srv.url(&format!("/api/v1/experiences/{topic}/{id}")))
        .json(&serde_json::json!({"when":"w2"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(patched["when"], "w2");

    // POST promote bumps good_count.
    let before = patched["good_count"].as_u64().unwrap();
    let promoted: Value = client
        .post(srv.url(&format!("/api/v1/experiences/{topic}/{id}/promote")))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(promoted["good_count"].as_u64().unwrap(), before + 1);

    // DELETE => 204; subsequent PATCH => 404.
    let del = client
        .delete(srv.url(&format!("/api/v1/experiences/{topic}/{id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status().as_u16(), 204);
    let gone = client
        .patch(srv.url(&format!("/api/v1/experiences/{topic}/{id}")))
        .json(&serde_json::json!({"when":"x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status().as_u16(), 404);
}

#[tokio::test]
async fn error_mapping() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();

    // Invalid add (missing check) => 400.
    let r = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({"topic":"t","when":"w","if":"i","do":"d"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);

    // Missing record => 404.
    let r = client
        .patch(srv.url("/api/v1/experiences/no/such"))
        .json(&serde_json::json!({"when":"x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 404);

    // Clear without confirm => 400.
    let r = client
        .delete(srv.url("/api/v1/experiences?all=true"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
}

#[tokio::test]
async fn pagination_clamped() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let r = client
        .get(srv.url("/api/v1/experiences?limit=10000"))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
}

#[tokio::test]
async fn topics_list_search_and_level_zero() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    for topic in ["travel/hotel/checkout", "other/x"] {
        client
            .post(srv.url("/api/v1/experiences"))
            .json(&serde_json::json!({"topic":topic,"when":"w","if":"i","do":"d","check":"c"}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    // list: both sorted.
    let list: Value = client
        .get(srv.url("/api/v1/topics"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        list,
        serde_json::json!(["other/x", "travel/hotel/checkout"])
    );
    // level=1 truncates + dedups.
    let list: Value = client
        .get(srv.url("/api/v1/topics?level=1"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list, serde_json::json!(["other", "travel"]));
    // # pattern search (# URL-encoded as %23).
    let list: Value = client
        .get(srv.url("/api/v1/topics?q=travel/%23/checkout"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list, serde_json::json!(["travel/hotel/checkout"]));
    // level=0 => 400.
    let r = client
        .get(srv.url("/api/v1/topics?level=0"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
}

#[tokio::test]
async fn delete_missing_is_404_and_clear_requires_confirm() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;
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
    let mut child = tokio::process::Command::new(bin)
        .args(["--db", &db, "serve", "--bind", &bind])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if std::time::Instant::now() > deadline {
            panic!("server did not become ready");
        }
        if client
            .get(format!("{base}/healthz"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Delete on a never-existing id => 404.
    let r = client
        .delete(format!("{base}/api/v1/experiences/no/such"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 404);
    // MCP clear without confirm => error mentioning confirm.
    let init = client
        .post(format!("{base}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let session = init.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_string();
    let proto = init
        .headers()
        .get("mcp-protocol-version")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_else(|| "2025-06-18".to_string());
    let st = client
        .post(format!("{base}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .header("Mcp-Protocol-Version", &proto)
        .json(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(st.as_u16(), 202);
    let body = client
        .post(format!("{base}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .header("Mcp-Protocol-Version", &proto)
        .json(&serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"clear","arguments":{"all":true}}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        body.to_lowercase().contains("confirm"),
        "clear without confirm must fail: {body}"
    );
    let _ = child.kill().await;
}
