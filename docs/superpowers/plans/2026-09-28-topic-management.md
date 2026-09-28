# Topic Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enforce hierarchical topics (`travel/hotel/checkout`, segments `[a-z0-9_-]`), add MQTT-style `#` multi-level wildcard to experience search/export, and add paginated topic list/search on CLI, REST, web, and MCP.

**Architecture:** New pure `domain::topics` module (`validate_topic`, `validate_topic_pattern`, `topic_matches`, `truncate_topic`) consumed by `MemoryService`; service keeps fast exact-SQL path when pattern has no `#` and filters in Rust when it does; `ExperienceRepo::distinct_topics(deep)` supplies the listing base; all four surfaces are thin adapters over `MemoryService::list_topics`.

**Tech Stack:** Rust (edition 2021), rusqlite (bundled FTS5), clap 4 (derive), axum 0.7, askama 0.12, rmcp 3, serde/serde_json.

**Spec:** `docs/superpowers/specs/2026-09-28-topic-management-design.md`

## Global Constraints

- All behavior lives in `application::MemoryService` — adapters contain no SQL, lifecycle transitions, or ranking logic.
- Compound key `(topic, id)`; write validation failures are `AppError::InvalidInput` (CLI exit 2 / REST 400 / web 400 / MCP `invalid_params`).
- Explicit `(topic, id)` reads (`get/modify/promote/downgrade/delete`) do NO topic validation — miss/deleted reads as `NotFound`.
- Bare `travel` matches only exact `travel`; descendants require `travel/#`.
- `#` matches zero or more levels; `#` alone matches everything; `#` is full-segment only, never stored.
- Search/export visibility unchanged: `deleted`/`forgotten` always hidden, `inactive` only with `deep=true`.
- Wildcard only on search/export `--topic`; `clear --topic` stays exact-match and rejects `#`.
- Pagination: `limit` default 20, clamped to `MAX_LIMIT=100`; `limit=0`/over-range `offset` → `[]`.
- SQLite INTEGER epoch timestamps; wire uses `when/if/do/check`, RFC 3339 (unchanged).
- Reconcile before every op.
- Destructive actions need explicit confirmation; all HTML escaped (Askama auto-escape ON).
- Verification before every commit:
  `cargo fmt --check`, `cargo check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`.
- Behavior changes require updating the matching `docs/*.md` and tests.

## Review Focus

1. **`#` in destructive/clear paths** — `clear --topic 'travel/#'` must fail 400/exit-2, never wipe descendants. Test pinned in Task 2.
2. **Middle-`#` on semantic path** — `travel/#/checkout` with fake embedding provider must return only matching topics, never silently text-only. Test pinned in Task 2.
3. **Level applied after query match** — `topic search hot --level 1` over `travel/hotel` must return `travel` (query matches full topic, truncation after). Test pinned in Task 2.
4. **XSS via hierarchical topic** — a topic is now rendered on `/topics`; `"><script>` can never be stored (validation), but legacy/edge strings must render escaped. Test pinned in Task 5.
5. **REST `level=0` and MCP missing query** — `GET /api/v1/topics?level=0` → 400; `topics_search` without `query` → `invalid_params`. Tests pinned in Tasks 4 and 6.

---

## File Structure

- `src/domain/topics.rs` (new) — `validate_topic`, `validate_topic_pattern`, `topic_matches`, `truncate_topic`. Pure, no I/O.
- `src/domain/mod.rs`, `src/domain/commands.rs` — re-export module; add `TopicQuery { query: Option<String>, level: Option<u32>, limit: u32, offset: u32, deep: bool }` (+ `Default`: limit 20).
- `src/application/service.rs`, `src/application/mod.rs` — validation call sites, wildcard search filtering, `list_topics(&self, q: &TopicQuery) -> Result<Vec<String>, AppError>`; re-export `TopicQuery`.
- `src/storage/repository.rs` — `ExperienceRepo::distinct_topics(&self, deep: bool)` + `SqliteRepo` impl (two SQL strings); service-side level/query/pagination.
- `src/surfaces/cli/commands.rs`, `src/surfaces/cli/mod.rs` — `Commands::Topic(TopicArgs)` with `TopicList`/`TopicSearch` subcommands; shared `--topic` help text constant; dispatch in `run_cli`.
- `src/main.rs` — extend the `run_cli` dispatch match arm to include `Commands::Topic(_)`.
- `src/surfaces/http/rest.rs` — `GET /api/v1/topics` handler + route.
- `src/surfaces/http/web.rs` — `GET /topics` handler + route + index link.
- `src/surfaces/http/mcp.rs` — `topics_list`, `topics_search` tools.
- `tests/domain_test.rs` (extend), `tests/application_test.rs` (extend), `tests/storage_test.rs` (extend), `tests/cli_test.rs`, `tests/rest_test.rs`, `tests/web_test.rs`, `tests/mcp_test.rs` + `tests/mcp_stdio_test.rs` (extend).
- Docs: `docs/cli.md`, `docs/rest-api.md`, `docs/web-server.md`, `docs/mcp.md`, `README.md`, `docs/architecture.md`, `AGENTS.md`.

---

### Task 1: Domain topics module + TopicQuery

**Files:**
- Create: `src/domain/topics.rs`
- Modify: `src/domain/mod.rs`
- Modify: `src/domain/commands.rs`
- Test: `tests/domain_test.rs`

**Interfaces:**
- Consumes: `crate::error::AppError::InvalidInput` for failures.
- Produces:
  - `pub fn validate_topic(topic: &str) -> Result<(), AppError>` in `crate::domain::topics`
  - `pub fn validate_topic_pattern(pattern: &str) -> Result<(), AppError>` in `crate::domain::topics`
  - `pub fn topic_matches(pattern: &str, topic: &str) -> bool` in `crate::domain::topics`
  - `pub fn truncate_topic(topic: &str, level: u32) -> String` in `crate::domain::topics`
  - `pub struct TopicQuery { pub query: Option<String>, pub level: Option<u32>, pub limit: u32, pub offset: u32, pub deep: bool }` in `crate::domain::commands` (+ `Default`: `query: None, level: None, limit: 20, offset: 0, deep: false`)

- [ ] **Step 1: Write failing tests in `tests/domain_test.rs`**

```rust
#[test]
fn topic_validation_accepts_hierarchy() {
    for t in ["travel", "travel/hotel/checkout", "a-b/c_d/e9"] {
        assert!(ilearned::domain::topics::validate_topic(t).is_ok(), "{t}");
    }
}

#[test]
fn topic_validation_rejects_bad_form() {
    for t in ["", "/", "travel/", "/travel", "a//b", "Travel", "a.b", "a b", "a#", "trav#", "a/b/"] {
        assert!(ilearned::domain::topics::validate_topic(t).is_err(), "{t}");
    }
}

#[test]
fn topic_pattern_allows_hash_segments() {
    for p in ["#", "travel/#", "#/checkout", "travel/#/checkout"] {
        assert!(ilearned::domain::topics::validate_topic_pattern(p).is_ok(), "{p}");
    }
    for p in ["trav#", "travel/#/"] {
        assert!(ilearned::domain::topics::validate_topic_pattern(p).is_err(), "{p}");
    }
}

#[test]
fn topic_matches_matrix() {
    use ilearned::domain::topics::topic_matches as m;
    assert!(m("travel/#", "travel"));
    assert!(m("travel/#", "travel/hotel/checkout"));
    assert!(!m("travel/#", "other/x"));
    assert!(m("#", "anything/at/all"));
    assert!(m("#/checkout", "travel/hotel/checkout"));
    assert!(m("travel/#/checkout", "travel/hotel/checkout"));
    assert!(m("travel/#/checkout", "travel/checkout"));
    assert!(!m("travel", "travel/hotel")); // bare = exact only
    assert!(m("travel", "travel"));
}

#[test]
fn truncate_topic_applies_level() {
    assert_eq!(ilearned::domain::topics::truncate_topic("travel/hotel/checkout", 2), "travel/hotel");
    assert_eq!(ilearned::domain::topics::truncate_topic("travel", 5), "travel");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test domain_test topic_`
Expected: FAIL — `ilearned::domain::topics` module not found.

- [ ] **Step 3: Implement `src/domain/topics.rs` + wire `mod.rs` + add `TopicQuery` to `commands.rs`**

Segment rule: split on `/`; every segment non-empty and all bytes in `[a-z0-9_-]`; pattern form additionally accepts a segment equal to exactly `#`. `topic_matches`: recursive matcher over pattern/topic segment slices where `#` matches zero or more segments. `truncate_topic`: `topic.split('/').take(level as usize).collect::<Vec<_>>().join("/")`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test domain_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/domain/topics.rs src/domain/mod.rs src/domain/commands.rs tests/domain_test.rs
git commit -m "feat: add hierarchical topic validation and matcher"
```

---

### Task 2: Service validation + wildcard search + list_topics (+ repo distinct_topics)

**Files:**
- Modify: `src/storage/repository.rs`
- Modify: `src/application/service.rs`
- Modify: `src/application/mod.rs`
- Test: `tests/application_test.rs`, `tests/storage_test.rs`

**Interfaces:**
- Consumes: `crate::domain::topics::{validate_topic, validate_topic_pattern, topic_matches, truncate_topic}`, `crate::domain::commands::TopicQuery`.
- Produces: `pub fn list_topics(&self, q: &TopicQuery) -> Result<Vec<String>, AppError>` on `MemoryService<R>`; `fn distinct_topics(&self, deep: bool) -> Result<Vec<String>, AppError>` on `ExperienceRepo`.

- [ ] **Step 1: Write failing tests**

In `tests/storage_test.rs`:

```rust
#[test]
fn distinct_topics_respects_visibility() {
    // seed active + inactive + deleted + forgotten topics; assert
    // deep=false returns only active topics sorted; deep=true adds inactive only
}
```

In `tests/application_test.rs`:

```rust
#[test]
fn add_rejects_invalid_topic() {
    // add with topic "Travel/Hotel" => Err(InvalidInput)
}

#[test]
fn search_wildcard_middle_hash() {
    // seed travel/hotel/checkout + travel/flight/checkout + other/x;
    // search topic "travel/#/checkout" => exactly the two travel records
}

#[test]
fn search_bare_topic_is_exact_only() {
    // seed travel + travel/hotel; search topic "travel" => only travel record
}

#[test]
fn list_topics_level_query_pagination() {
    // seed travel/hotel/checkout, travel/hotel/lobby, other/x
    // list level=Some(2) => ["other/x", "travel/hotel"]
    // search query "hot" level=Some(1) => ["travel"] (match-before-truncate)
    // level=Some(0) => Err(InvalidInput)
}

#[test]
fn clear_rejects_wildcard_topic() {
    // clear Topic("travel/#") => Err(InvalidInput)
}
```

For the semantic wildcard pin (Review Focus 2), extend with the fake provider:

```rust
#[test]
fn semantic_search_respects_middle_hash() {
    // service with deterministic fake provider; seed travel/hotel/checkout + other/x
    // search { topic: Some("travel/#/checkout"), semantic: Some("q") } => only travel record
}
```

(Follow the existing fake-provider setup pattern in `application_test.rs`.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test application_test search_wildcard_middle_hash; cargo test --test storage_test distinct_topics_respects_visibility`
Expected: FAIL — methods/validation missing.

- [ ] **Step 3: Implement repo + service**

`repository.rs`: add `fn distinct_topics(&self, deep: bool)` to the trait; `SqliteRepo` impl uses `SELECT DISTINCT topic FROM experiences WHERE state='active' ORDER BY topic` (deep=false) and `... WHERE state IN ('active','inactive') ...` (deep=true).

`service.rs`:
- `add`: `validate_topic(&cmd.topic)?` first.
- `validate_import`: `validate_topic(&e.topic)?` first.
- `clear`: on `ClearCommand::Topic(t)` run `validate_topic(t)?` (rejects `#` since hash is pattern-only).
- `search`/`export`: topic filter validated with `validate_topic_pattern`; compute `has_wildcard = topic.as_deref().is_some_and(|t| t.split('/').any(|s| s == "#"))`; when true, call repo with `None` topic and apply `.filter(|e| topic_matches(pat, &e.topic))` before the existing `visible` filter on browse/FTS/semantic paths.
- New `list_topics`: `self.reconcile()?`; `if q.level == Some(0) => InvalidInput`; `let base = self.repo.distinct_topics(q.deep)?`; filter by query (blank/None = all; `#`-containing = `validate_topic_pattern` + `topic_matches` on full topic; else substring `contains`); apply `level` truncation + dedup; sort; paginate with `q.limit.min(MAX_LIMIT)`.
- `application/mod.rs`: re-export `TopicQuery`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test application_test --test storage_test --test domain_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/storage/repository.rs src/application/service.rs src/application/mod.rs tests/application_test.rs tests/storage_test.rs
git commit -m "feat: validate topics, wildcard search, list_topics"
```

---

### Task 3: CLI `topic` command

**Files:**
- Modify: `src/surfaces/cli/commands.rs`
- Modify: `src/surfaces/cli/mod.rs`
- Modify: `src/main.rs`
- Test: `tests/cli_test.rs`

**Interfaces:**
- Consumes: `MemoryService::list_topics(&TopicQuery)`.
- Produces: `Commands::Topic(TopicArgs)`; `ilearned topic list [--level N] [--limit N] [--offset N] [--deep]`, `ilearned topic search QUERY [--level N] [--limit N] [--offset N] [--deep]`.

- [ ] **Step 1: Write failing CLI tests in `tests/cli_test.rs`**

```rust
#[test]
fn topic_list_and_search_json() {
    // seed travel/hotel/checkout + travel/flight via binary; run
    // `topic list --level 1 --json` => ["travel"]; run
    // `topic search 'travel/#' --json` => both full topics sorted
}
```

(Follow the existing `cli_test.rs` binary-invocation pattern.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test cli_test topic_`
Expected: FAIL — no `topic` subcommand.

- [ ] **Step 3: Implement CLI**

`commands.rs`: shared help-text constant, e.g. `pub const TOPIC_HELP: &str = "hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; search accepts # multi-level wildcard"`; apply to every `--topic` arg; `search --topic` help appends `# examples: travel/#, #/checkout`. Add:

```rust
pub enum TopicCommands { List(TopicListArgs), Search(TopicSearchArgs) }
```

with `limit` default 20 / `offset` default 0 / `deep` / `level: Option<u32>` on both, plus positional `query: String` on search. `run_cli` handles `Commands::Topic`: build `TopicQuery`, call `svc.list_topics`, render one topic per line (human) or JSON array (`--json`).

`main.rs`: extend the `run_cli` dispatch arm (`Commands::Add(_) | ...`) with `Commands::Topic(_)`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test cli_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/cli/commands.rs src/surfaces/cli/mod.rs src/main.rs tests/cli_test.rs
git commit -m "feat: add topic list/search CLI commands"
```

---

### Task 4: REST `GET /api/v1/topics`

**Files:**
- Modify: `src/surfaces/http/rest.rs`
- Test: `tests/rest_test.rs`

**Interfaces:**
- Consumes: `MemoryService::list_topics`.
- Produces: `GET /api/v1/topics?level=&q=&limit=&offset=&deep=` → `200 ["topic", ...]`; `level=0`/bad pattern → 400 `{"error": ...}`.

- [ ] **Step 1: Write failing test in `tests/rest_test.rs`**

```rust
#[tokio::test]
async fn topics_list_search_and_level_zero() {
    // seed travel/hotel/checkout + other/x; GET /api/v1/topics => both sorted;
    // GET /api/v1/topics?level=1 => ["other", "travel"];
    // GET /api/v1/topics?q=travel/%23/checkout => ["travel/hotel/checkout"];
    // GET /api/v1/topics?level=0 => 400
}
```

(Follow the existing axum test-server pattern in `rest_test.rs`. `#` must be URL-encoded as `%23` in `q`.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test rest_test topics_`
Expected: FAIL — no route.

- [ ] **Step 3: Implement handler + route**

```rust
pub struct TopicParams { pub level: Option<u32>, pub q: Option<String>, pub limit: Option<u32>, pub offset: Option<u32>, pub deep: Option<bool> }
pub async fn topics<R: ExperienceRepo + VectorStore>(State(svc): State<Shared<R>>, Query(p): Query<TopicParams>) -> Result<Json<Vec<String>>, ApiError>
```

Builds `TopicQuery` (`limit.unwrap_or(20)`, `offset.unwrap_or(0)`, `deep.unwrap_or(false)`), calls `svc.list_topics`. Register `.route("/api/v1/topics", get(topics::<R>))` in `rest_routes`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test rest_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/http/rest.rs tests/rest_test.rs
git commit -m "feat: add GET /api/v1/topics"
```

---

### Task 5: Web `GET /topics`

**Files:**
- Modify: `src/surfaces/http/web.rs`
- Test: `tests/web_test.rs`

**Interfaces:**
- Consumes: `MemoryService::list_topics`.
- Produces: `GET /topics?level=&q=&limit=&offset=&deep=` HTML page with level/q form + escaped result list; index `/` links to `/topics`.

- [ ] **Step 1: Write failing test in `tests/web_test.rs`**

```rust
#[tokio::test]
async fn topics_page_lists_and_escapes() {
    // seed travel/hotel; GET /topics => 200, body contains "travel/hotel" and link from /;
    // GET /topics?q=travel => contains travel/hotel
}
```

Plus an escaping pin: seed via direct repo insert a topic containing `<script>` markup text (bypasses validation, simulates legacy row) and assert the `/topics` body contains `&lt;script&gt;` and no raw `<script>`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test web_test topics_`
Expected: FAIL — no route.

- [ ] **Step 3: Implement page + route**

Askama inline `TopicsTemplate { q: String, level: String, deep: bool, results: Vec<String> }` with a `level`/`q` form; handler builds `TopicQuery` (parse `level: Option<String>` → `Option<u32>`, invalid number → 400 page); register `.route("/topics", get(topics_page::<R>))`; add `<a href="/topics">topics</a>` link to the index template.

- [ ] **Step 4: Run tests**

Run: `cargo test --test web_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/http/web.rs tests/web_test.rs
git commit -m "feat: add /topics web page"
```

---

### Task 6: MCP `topics_list` + `topics_search`

**Files:**
- Modify: `src/surfaces/http/mcp.rs`
- Test: `tests/mcp_test.rs`, `tests/mcp_stdio_test.rs`

**Interfaces:**
- Consumes: `MemoryService::list_topics`.
- Produces: tools `topics_list{level?,limit?,offset?,deep?}` and `topics_search{query,level?,limit?,offset?,deep?}` returning JSON string arrays; tool count 7 → 9.

- [ ] **Step 1: Write failing tests**

In `tests/mcp_stdio_test.rs` (follow existing stdio handshake + `tools/call` pattern):

```python
# call topics_list after seeding travel/hotel/checkout => ["travel/hotel/checkout"]
# call topics_search {query: "travel/#"} => same; call topics_search {} => invalid_params
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test mcp_stdio_test topics_`
Expected: FAIL — unknown tools.

- [ ] **Step 3: Implement tools**

```rust
pub struct TopicsListArgs { pub level: Option<u32>, pub limit: Option<u32>, pub offset: Option<u32>, pub deep: Option<bool> }
pub struct TopicsSearchArgs { pub query: String, pub level: Option<u32>, pub limit: Option<u32>, pub offset: Option<u32>, pub deep: Option<bool> }
```

`#[tool(description = "List existing topics ...")] fn topics_list(...)` and `#[tool(description = "Search topics by substring or # pattern ...")] fn topics_search(...)`; both map errors via existing `map_err`. Update the `ServerHandler` instructions string to mention topic listing.

- [ ] **Step 4: Run tests**

Run: `cargo test --test mcp_test --test mcp_stdio_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/http/mcp.rs tests/mcp_test.rs tests/mcp_stdio_test.rs
git commit -m "feat: add topics_list/topics_search MCP tools"
```

---

### Task 7: Docs + full verification

**Files:**
- Modify: `docs/cli.md`, `docs/rest-api.md`, `docs/web-server.md`, `docs/mcp.md`, `README.md`, `docs/architecture.md`, `AGENTS.md`

**Interfaces:**
- Consumes: final implemented behavior from Tasks 1–6.
- Produces: docs matching implementation; clean `fmt/check/test/clippy`.

- [ ] **Step 1: Update `docs/cli.md`**

Topic form + `#` examples; `topic list/search` usage lines and flags; note `export --topic` accepts wildcards while `clear --topic` stays exact.

- [ ] **Step 2: Update `docs/rest-api.md`, `docs/web-server.md`, `docs/mcp.md`**

REST table row for `GET /api/v1/topics` + semantics paragraph; web routes row for `GET /topics`; MCP 7 → 9 tools with new arg schemas.

- [ ] **Step 3: Update `README.md`, `docs/architecture.md`, `AGENTS.md`**

README CLI usage block + REST table + MCP tool count; architecture key-invariant + application-method mention; AGENTS.md topic-form invariant line.

- [ ] **Step 4: Run full verification**

Run:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: all four pass.

- [ ] **Step 5: Commit**

```bash
git add docs/cli.md docs/rest-api.md docs/web-server.md docs/mcp.md README.md docs/architecture.md AGENTS.md
git commit -m "docs: document hierarchical topics and wildcard search"
```

## Self-Review

1. **Spec coverage:** every spec section has an owning task — form/validation + matcher (T1), service validation + wildcard + `list_topics` (T2), CLI (T3), REST (T4), web (T5), MCP (T6), docs + verification (T7). Export wildcard piggybacks on the T2 `search`/`export` filter change and is documented in T7.
2. **Step scan:** each step names one artifact (test names + assertions as code; signatures with file paths; exact run commands with expected output). No bodies beyond the matcher/validation rules and SQL strings the signatures alone don't determine.
3. **Type consistency:** `TopicQuery` fields (`query/level/limit/offset/deep`) are identical in T1–T6; `list_topics(&TopicQuery) -> Result<Vec<String>, AppError>` and `distinct_topics(deep: bool) -> Result<Vec<String>, AppError>` signatures match across tasks; CLI/REST/web/MCP arg names mirror `TopicQuery`.
4. **Review Focus:** all five lines have owning-task pins (clear-`#` → T2; semantic middle-`#` → T2; match-before-truncate → T2; XSS → T5; `level=0`/missing query → T4/T6).
5. **Proportion:** plan states decisions (names, signatures, SQL, help text, routes) without transcribing implementations; service internals stay prose.
