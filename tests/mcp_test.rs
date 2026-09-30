use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde_json::Value;

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

struct McpClient {
    client: reqwest::Client,
    srv: TestServer,
    session: String,
    proto: String,
    next_id: u64,
}

impl McpClient {
    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let body = self
            .client
            .post(self.srv.url("/mcp"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("mcp-session-id", &self.session)
            .header("Mcp-Protocol-Version", &self.proto)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        for line in body.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if v.get("id") == Some(&serde_json::json!(id)) {
                        return v;
                    }
                }
            }
        }
        serde_json::from_str(&body).expect("MCP response must be JSON or SSE")
    }

    async fn call(&mut self, name: &str, args: Value) -> Value {
        self.rpc(
            "tools/call",
            serde_json::json!({"name": name, "arguments": args}),
        )
        .await
    }
}

fn tool_json(response: &Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("MCP tool result must contain JSON text");
    serde_json::from_str(text).expect("MCP tool result text must be JSON")
}

async fn spawn_client() -> McpClient {
    let dir = tempfile::tempdir().unwrap();
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"));
    let db = dir.path().join("t.db");
    let config = dir.path().join("config.toml");
    std::fs::write(&config, format!("db = {:?}\n", db.to_string_lossy())).unwrap();
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
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // MCP initialize handshake: initialize → capture session id + protocol
    // version → notifications/initialized.
    let init_resp = client
        .post(srv.url("/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let session = init_resp.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_string();
    let proto = init_resp
        .headers()
        .get("mcp-protocol-version")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_else(|| "2025-06-18".to_string());
    let st = client
        .post(srv.url("/mcp"))
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
    McpClient {
        client,
        srv,
        session,
        proto,
        next_id: 0,
    }
}

#[tokio::test]
async fn tool_list_has_nine_tools() {
    let mut mcp = spawn_client().await;
    let resp = mcp.rpc("tools/list", serde_json::json!({})).await;
    let tools = resp["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "add",
            "clear",
            "delete",
            "demote",
            "promote",
            "search",
            "topics_list",
            "topics_search",
            "update"
        ]
    );
}

#[tokio::test]
async fn add_then_search_via_tools() {
    let mut mcp = spawn_client().await;
    let added = mcp
        .call(
            "add",
            serde_json::json!({"topic":"mcp","when":"w","if":"i","do":"d","check":"c"}),
        )
        .await;
    assert!(added.get("result").is_some(), "add failed: {added}");
    let added_json = tool_json(&added);
    assert_eq!(added_json["added"]["topic"], "mcp");
    let found = mcp
        .call("search", serde_json::json!({"topic": "mcp"}))
        .await;
    let text = serde_json::to_string(&found["result"]).unwrap();
    assert!(text.contains("mcp"), "search should find record: {found}");
}

#[tokio::test]
async fn mutation_tools_return_enveloped_results() {
    let mut mcp = spawn_client().await;
    let added = tool_json(
        &mcp.call(
            "add",
            serde_json::json!({"topic":"mcp","when":"w","if":"i","do":"d","check":"c"}),
        )
        .await,
    );
    let id = added["added"]["id"].as_str().unwrap().to_string();
    assert_eq!(added["added"]["topic"], "mcp");

    let modified = tool_json(
        &mcp.call(
            "update",
            serde_json::json!({"topic":"mcp","id":id,"when":"updated"}),
        )
        .await,
    );
    assert_eq!(
        modified,
        serde_json::json!({"modified": {"topic": "mcp", "id": id}})
    );

    let promoted = tool_json(
        &mcp.call("promote", serde_json::json!({"topic":"mcp","id":id}))
            .await,
    );
    assert_eq!(
        promoted,
        serde_json::json!({
            "modified": {"topic": "mcp", "id": id, "good_count": 2, "bad_count": 0, "state": "active"}
        })
    );

    let downgraded = tool_json(
        &mcp.call("demote", serde_json::json!({"topic":"mcp","id":id}))
            .await,
    );
    assert_eq!(
        downgraded,
        serde_json::json!({
            "modified": {"topic": "mcp", "id": id, "good_count": 2, "bad_count": 1, "state": "active"}
        })
    );

    let deleted = tool_json(
        &mcp.call("delete", serde_json::json!({"topic":"mcp","id":id}))
            .await,
    );
    assert_eq!(
        deleted,
        serde_json::json!({"deleted": {"topic": "mcp", "id": id}})
    );

    for topic in ["clear-a", "clear-a", "clear-b"] {
        let result = tool_json(
            &mcp.call(
                "add",
                serde_json::json!({"topic":topic,"when":"w","if":"i","do":"d","check":"c"}),
            )
            .await,
        );
        assert_eq!(result["added"]["topic"], topic);
    }

    let cleared = tool_json(
        &mcp.call("clear", serde_json::json!({"all":true,"confirm":true}))
            .await,
    );
    assert_eq!(
        cleared,
        serde_json::json!({
            "cleared": {"num_of_topics": 2, "num_of_items": 3}
        })
    );
}

#[tokio::test]
async fn typed_errors_propagate() {
    let mut mcp = spawn_client().await;
    let resp = mcp
        .call(
            "update",
            serde_json::json!({"topic":"no","id":"such","when":"x"}),
        )
        .await;
    let text = serde_json::to_string(&resp).unwrap();
    assert!(
        text.to_lowercase().contains("not found"),
        "missing update must surface not-found: {resp}"
    );
}
