use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use tempfile::TempDir;

fn bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_ilearned"))
}

struct StdioChild {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl StdioChild {
    fn spawn(args: &[&str], cwd: &std::path::Path) -> Self {
        let mut child = Command::new(bin())
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn ilearned mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        Self {
            child,
            stdin,
            reader,
            next_id: 0,
        }
    }

    fn send(&mut self, v: &serde_json::Value) {
        let line = serde_json::to_string(v).unwrap();
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> serde_json::Value {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).unwrap();
        assert!(n > 0, "server closed stdout unexpectedly");
        serde_json::from_str(line.trim()).expect("server must emit JSON-RPC lines")
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let v = self.recv();
            if v.get("id") == Some(&serde_json::json!(id)) {
                return v;
            }
            // Skip notifications / unrelated messages.
        }
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.send(&serde_json::json!({"jsonrpc":"2.0","method":method,"params":params}));
    }

    fn shutdown(mut self) {
        drop(self.stdin);
        // Wait briefly, then kill.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait().unwrap() {
                Some(_) => break,
                None if std::time::Instant::now() > deadline => {
                    self.child.kill().ok();
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

fn handshake(c: &mut StdioChild) {
    let resp = c.request(
        "initialize",
        serde_json::json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}),
    );
    assert!(
        resp.get("result").is_some(),
        "initialize must succeed: {resp}"
    );
    c.notify("notifications/initialized", serde_json::json!({}));
}

#[test]
fn stdio_lists_seven_tools() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("t.db").to_string_lossy().to_string();
    let mut c = StdioChild::spawn(&["--db", &db, "mcp"], dir.path());
    handshake(&mut c);
    let resp = c.request("tools/list", serde_json::json!({}));
    let tools = resp["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "add",
            "clear",
            "delete",
            "downgrade",
            "modify",
            "promote",
            "search",
            "topics_list",
            "topics_search"
        ]
    );
    c.shutdown();
}

#[test]
fn stdio_add_then_search_roundtrip() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("t.db").to_string_lossy().to_string();
    let mut c = StdioChild::spawn(&["--db", &db, "mcp"], dir.path());
    handshake(&mut c);
    let added = c.request(
        "tools/call",
        serde_json::json!({"name":"add","arguments":{"topic":"std","when":"w","if":"i","do":"d","check":"c"}}),
    );
    assert!(added.get("result").is_some(), "add must succeed: {added}");
    let found = c.request(
        "tools/call",
        serde_json::json!({"name":"search","arguments":{"topic":"std"}}),
    );
    let text = serde_json::to_string(&found["result"]).unwrap();
    assert!(text.contains("std"), "search should find record: {found}");
    c.shutdown();
}

#[test]
fn stdio_defaults_db_to_dot_ilearned_dir() {
    let dir = TempDir::new().unwrap();
    // No --db flag: server must persist to ./.ilearned/ilearned.db under cwd.
    let mut c = StdioChild::spawn(&["mcp"], dir.path());
    handshake(&mut c);
    let added = c.request(
        "tools/call",
        serde_json::json!({"name":"add","arguments":{"topic":"d","when":"w","if":"i","do":"d","check":"c"}}),
    );
    assert!(added.get("result").is_some(), "add must succeed: {added}");
    c.shutdown();
    assert!(
        dir.path().join(".ilearned").join("ilearned.db").exists(),
        "./.ilearned/ilearned.db must be created under the project dir"
    );
}

#[test]
fn stdio_topics_list_and_search() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("t.db").to_string_lossy().to_string();
    let mut c = StdioChild::spawn(&["--db", &db, "mcp"], dir.path());
    handshake(&mut c);
    let added = c.request(
        "tools/call",
        serde_json::json!({"name":"add","arguments":{"topic":"travel/hotel/checkout","when":"w","if":"i","do":"d","check":"c"}}),
    );
    assert!(added.get("result").is_some(), "add must succeed: {added}");
    // topics_list returns the full topic.
    let listed = c.request(
        "tools/call",
        serde_json::json!({"name":"topics_list","arguments":{}}),
    );
    let text = serde_json::to_string(&listed["result"]).unwrap();
    assert!(text.contains("travel/hotel/checkout"), "list: {listed}");
    // topics_search with a # pattern returns it too.
    let found = c.request(
        "tools/call",
        serde_json::json!({"name":"topics_search","arguments":{"query":"travel/#"}}),
    );
    let text = serde_json::to_string(&found["result"]).unwrap();
    assert!(text.contains("travel/hotel/checkout"), "search: {found}");
    // topics_search without query => error result (isError).
    let missing = c.request(
        "tools/call",
        serde_json::json!({"name":"topics_search","arguments":{}}),
    );
    let text = serde_json::to_string(&missing).unwrap();
    assert!(
        text.contains("isError") || missing.get("error").is_some(),
        "missing query must error: {missing}"
    );
    c.shutdown();
}
