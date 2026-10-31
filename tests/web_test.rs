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

    for column in ["Topic", "When", "If", "Do", "Check"] {
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
    assert!(
        index.contains(".ops{width:5%"),
        "Ops should use 5% of the table width"
    );
    assert!(index.contains(r#"<th class="ops">Ops</th>"#));

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
    for operation in ["promote", "demote"] {
        assert!(index.contains(&format!(
            r#"action="/experiences/alpha/{}/{}?q=&amp;topic=&amp;deep=false""#,
            alpha_id, operation
        )));
    }
    assert!(index.contains(&format!(
        r#"action="/experiences/alpha/{}/delete""#,
        alpha_id
    )));

    let alpha_row_start = index
        .find(&format!(r#"href="/experiences/alpha/{}""#, alpha_id))
        .expect("alpha detail link is present");
    let ops_start = alpha_row_start
        + index[alpha_row_start..]
            .find(r#"<td class="ops">"#)
            .expect("alpha operations cell is present");
    let ops_end = ops_start
        + index[ops_start..]
            .find("</td>")
            .expect("operations cell is closed");
    let alpha_ops = &index[ops_start..ops_end];
    let ops_lines: Vec<_> = alpha_ops
        .split(r#"<div class="ops-line">"#)
        .skip(1)
        .map(|line| line.split("</div>").next().unwrap())
        .collect();
    assert_eq!(ops_lines.len(), 3, "operations should occupy three lines");
    let delete_position = ops_lines[0].find('✘').expect("delete icon is present");
    let edit_position = ops_lines[0].find('✎').expect("edit icon is present");
    assert!(
        delete_position < edit_position,
        "delete should precede edit"
    );
    assert!(ops_lines[0].contains(&format!("/experiences/alpha/{}/edit", alpha_id)));
    assert!(ops_lines[0].contains(&format!("/experiences/alpha/{}/delete", alpha_id)));
    assert!(ops_lines[1].contains(&format!("/experiences/alpha/{}/promote", alpha_id)));
    assert!(ops_lines[1].contains(r#">★</button>"#));
    assert!(ops_lines[1].contains(r#"class="op-count" aria-label="Good count: 1">1</span>"#));
    assert!(ops_lines[2].contains(&format!("/experiences/alpha/{}/demote", alpha_id)));
    assert!(ops_lines[2].contains(r#">☆</button>"#));
    assert!(ops_lines[2].contains(r#"class="op-count" aria-label="Bad count: 0">0</span>"#));
}

#[tokio::test]
async fn index_truncates_experience_fields_at_110_characters() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let when = "a".repeat(110);
    let if_text = "猫".repeat(111);
    let do_text = "c".repeat(111);
    let check_text = "d".repeat(111);

    client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({
            "topic": "truncate",
            "when": when,
            "if": if_text,
            "do": do_text,
            "check": check_text
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

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

    assert!(
        index.contains(&format!("{}…", "猫".repeat(110))),
        "fields over the limit should end with an ellipsis"
    );
    for (value, expected) in [
        ("a".repeat(110), "a".repeat(110)),
        ("猫".repeat(111), format!("{}…", "猫".repeat(110))),
        ("c".repeat(111), format!("{}…", "c".repeat(110))),
        ("d".repeat(111), format!("{}…", "d".repeat(110))),
    ] {
        assert!(
            index.contains(&format!(r#"<td class="experience-text">{}</td>"#, expected)),
            "unexpected table display for a {}-character value",
            value.chars().count()
        );
    }
    assert!(index.contains(".experience-text{font-size:.85em}"));
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
        format!(
            r#"action="/experiences/nested%2Fchild/{}/promote?q=&amp;topic=&amp;deep=false""#,
            id
        ),
        format!(
            r#"action="/experiences/nested%2Fchild/{}/demote?q=&amp;topic=&amp;deep=false""#,
            id
        ),
        format!(r#"action="/experiences/nested%2Fchild/{}/delete""#, id),
    ] {
        assert!(
            index.contains(&path),
            "missing encoded action path: {}",
            path
        );
    }
}

#[tokio::test]
async fn feedback_actions_return_to_filtered_list_with_updated_counts() {
    let srv = spawn_server().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let created: serde_json::Value = client
        .post(srv.url("/api/v1/experiences"))
        .json(&serde_json::json!({
            "topic": "feedback",
            "when": "needle before action",
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
    let list_query = "q=needle&topic=feedback&deep=true";

    let index = client
        .get(srv.url(&format!("/?{}", list_query)))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    for action in ["promote", "demote"] {
        assert!(
            index.contains(&format!(
                r#"action="/experiences/feedback/{}/{}?q=needle&amp;topic=feedback&amp;deep=true""#,
                id, action
            )),
            "missing filtered form action; rendered index: {}",
            index
        );
    }

    let promoted = client
        .post(srv.url(&format!(
            "/experiences/feedback/{}/promote?{}",
            id, list_query
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(promoted.status(), reqwest::StatusCode::SEE_OTHER);
    let promoted_location = promoted.headers()[reqwest::header::LOCATION]
        .to_str()
        .unwrap();
    assert_eq!(promoted_location, format!("/?{}", list_query));
    let promoted_list = client
        .get(srv.url(promoted_location))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(promoted_list.contains(r#"<th class="ops">Ops</th>"#));
    assert!(promoted_list.contains("Good count: 2"));
    assert!(promoted_list.contains("Bad count: 0"));

    let demoted = client
        .post(srv.url(&format!(
            "/experiences/feedback/{}/demote?{}",
            id, list_query
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(demoted.status(), reqwest::StatusCode::SEE_OTHER);
    let demoted_location = demoted.headers()[reqwest::header::LOCATION]
        .to_str()
        .unwrap();
    assert_eq!(demoted_location, format!("/?{}", list_query));
    let demoted_list = client
        .get(srv.url(demoted_location))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(demoted_list.contains("Good count: 2"));
    assert!(demoted_list.contains("Bad count: 1"));
}

#[tokio::test]
async fn add_form_uses_wide_multiline_fields() {
    let srv = spawn_server().await;
    let client = reqwest::Client::new();
    let add = client
        .get(srv.url("/experiences/new"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(add.contains(r#"<form method="post" action="/experiences">"#));
    assert!(add.contains(r#"<input name="topic" required>"#));
    for (name, label) in [
        ("when_text", "when"),
        ("if_text", "if"),
        ("do_text", "do"),
        ("check", "check"),
    ] {
        assert!(
            add.contains(&format!(
                r#"<label>{} <textarea name="{}" required></textarea></label>"#,
                label, name
            )),
            "{} should be a required multiline field",
            label
        );
    }
    assert!(add.contains("max-width:96vw"));
    assert!(add.contains("textarea{display:block;width:100%"));
    assert!(add.contains(">Add</button>"));
}
