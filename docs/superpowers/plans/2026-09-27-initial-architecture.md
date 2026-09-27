# ilearned Initial Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first-release ilearned modular monolith: SQLite + FTS5 core, MemoryService, CLI + REST + server-rendered web + HTTP MCP in one binary, with optional OpenAI-compatible semantic search.

**Architecture:** `domain` (pure types/rules) <- `application::MemoryService` (sole entry for adapters) -> `storage` (rusqlite) + `embedding` (trait + HTTP impl). `surfaces/cli` and `surfaces/http` are thin adapters with no SQL, lifecycle, or ranking logic.

**Tech Stack:** Rust (edition 2021), rusqlite (bundled FTS5), clap 4 (derive), axum 0.7, tokio (full), serde/serde_json, chrono (serde), uuid (v4), thiserror, reqwest (json), askama (web templates), rmcp (streamable HTTP MCP).

**Spec:** `docs/superpowers/specs/2026-09-27-initial-architecture-design.md`

## Global Constraints

- One binary: CLI subcommands + `serve` mode; REST + web + MCP share one listener.
- Single-user local-first; no auth, no multi-user ownership.
- Default bind `127.0.0.1:8787`; default DB `./ilearned.db`; config precedence flags > env (`ILEARNED_*`) > defaults.
- Lifecycle defaults: active period 60d, forget period 120d, retention 60d (all configurable).
- RRF constant `k=60`; default pagination `limit=20 offset=0`; enforced `MAX_LIMIT=100`.
- Timestamps: SQLite INTEGER Unix epoch UTC; JSON/HTML RFC 3339.
- SQLite text columns `when_text, if_text, do_text, check_text`; wire/domain JSON names `when, if, do, check`.
- `add` sets `good_count=1, bad_count=0, state=active`; modify requires >=1 field.
- Search never returns `deleted`/`forgotten` (even `deep=true`); `inactive` only when `deep=true`.
- Embedding failure on `add`/`modify` never rolls back canonical write; semantic query with embedding failure returns typed error (never silent text fallback).
- Destructive CLI/web actions require explicit confirmation; all HTML escaped.
- REST errors: 400 invalid, 404 not-found (incl. deleted), 503 embedding-required-but-unavailable, 500 internal.

## Review Focus

1. **FTS5 syntax passthrough** — input `text: "NEAR(broken"` must yield typed 400/`InvalidFtsSyntax`, not 500 or panic. Test pinned in Task 2.
2. **Lifecycle boundary off-by-one** — record aged exactly 60d+1s goes inactive; exactly 120d+1s goes forgotten (strict `>`). Test pinned in Task 2.
3. **XSS via experience fields** — `when: "<script>alert(1)</script>"` must render escaped in web detail/search pages. Test pinned in Task 7.
4. **Pagination abuse** — `limit=0` returns `[]`; `limit=10000` clamps to 100; huge `offset` returns `[]`, not error. Tests pinned in Tasks 3 and 6.
5. **Embedding timeout on combined search** — `text` + `semantic` with hanging provider must fail typed (503), not silently return text-only. Test pinned in Task 4.

---

## File Structure

- `Cargo.toml` — single package, one `[[bin]]` (`src/main.rs`), lib root `src/lib.rs`.
- `src/domain/{mod,experience,commands,lifecycle}.rs` — pure types + transition predicates.
- `src/{error,config}.rs` — `AppError`, `Config`/`EmbeddingConfig` + load precedence.
- `src/storage/{mod,sqlite,repository,fts,lifecycle,embeddings}.rs` — migrations, CRUD, FTS5, reconcile/purge, vector rows.
- `src/embedding/{mod,provider,openai,fake}.rs` — `EmbeddingProvider` trait, HTTP impl, deterministic fake.
- `src/application/{mod,service,ranking}.rs` — `MemoryService`, cosine + RRF helpers.
- `src/surfaces/{mod,cli/{mod,commands,render},http/{mod,server,rest,web,mcp}}.rs` + `templates/*.html` (askama).
- `tests/{domain_test,storage_test,application_test,cli_test,rest_test,web_test,mcp_test}.rs`.
- `docs/{architecture,TODO,cli,web-server,rest-api,mcp,storage,development}.md` + root `AGENTS.md`.

---

### Task 1: Scaffold, domain, config, errors, docs skeleton

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/main.rs`
- Create: `src/error.rs`, `src/config.rs`
- Create: `src/domain/mod.rs`, `src/domain/experience.rs`, `src/domain/commands.rs`, `src/domain/lifecycle.rs`
- Create: `docs/architecture.md`, `docs/TODO.md`, `docs/cli.md`, `docs/web-server.md`, `docs/rest-api.md`, `docs/mcp.md`, `docs/storage.md`, `docs/development.md`
- Create: `AGENTS.md`
- Test: `tests/domain_test.rs`

**Interfaces:**
- Consumes: nothing (foundation).
- Produces:
  - `pub enum State { Active, Inactive, Deleted, Forgotten }` (serde lowercase).
  - `pub struct Experience { topic: String, id: String, #[serde(rename="when")] when_text: String, #[serde(rename="if")] if_text: String, #[serde(rename="do")] do_text: String, #[serde(rename="check")] check_text: String, updated_at: DateTime<Utc>, good_count: u64, bad_count: u64, state: State }`
  - `pub struct AddCommand { topic, when_text, if_text, do_text, check_text: String }` + `validate() -> Result<(), AppError>`
  - `pub struct ModifyCommand { topic, id: String, when_text, if_text, do_text, check_text: Option<String> }` + `has_updates() -> bool`
  - `pub struct SearchQuery { topic, text, semantic: Option<String>, limit: u32, offset: u32, deep: bool }`
  - `pub struct FeedbackCommand { topic, id: String }`, `pub enum ClearCommand { Topic(String), All }`
  - `pub enum AppError { InvalidInput(String), NotFound{topic:String,id:String}, EmbeddingUnavailable(String), InvalidFtsSyntax(String), Storage(String), Internal(String) }` (thiserror; `From<rusqlite::Error>` maps to `Storage`)
  - `pub struct LifecycleConfig { active_period_days: u64, forget_period_days: u64, retention_days: u64 }` + `Default (60,120,60)`
  - `pub fn is_eligible(state: &State, deep: bool) -> bool` in `domain::lifecycle` (false for Deleted/Forgotten; Inactive only if deep)
  - `pub struct Config { db_path: PathBuf, bind: SocketAddr, lifecycle: LifecycleConfig, embedding: Option<EmbeddingConfig> }` + `Config::load_from(cli_matches) -> Result<Self, AppError>`
  - `pub struct EmbeddingConfig { endpoint: String, model: String, api_key: String, dims: usize, timeout_secs: u64 }`

- [ ] **Step 1: Write the failing test**

```rust
// tests/domain_test.rs
#[test] fn add_rejects_empty_topic() { /* AddCommand{topic:"",..} .validate() == Err(InvalidInput(_)) */ }
#[test] fn modify_rejects_no_fields() { /* ModifyCommand{all None}.has_updates() == false */ }
#[test] fn eligibility_rules() { /* Deleted=>false; Forgotten=>false even deep; Inactive=>deep only; Active=>true */ }
#[test] fn experience_serde_uses_readme_names() { /* to_value has "when"/"if"/"do"/"check", no "when_text" */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test domain_test`
Expected: FAIL (no lib / types missing).

- [ ] **Step 3: Implement `Cargo.toml`, `error`, `config`, `domain/*`, `lib.rs`, `main.rs` stub**

`main.rs` prints version placeholder only (real dispatch in Task 5). Docs skeleton: copy behavior statements verbatim from spec sections (no new claims); `TODO.md` lists this plan's 9 tasks as checkboxes; `AGENTS.md` points to README + docs/ + spec and lists `cargo fmt --check`, `cargo check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test domain_test && cargo fmt --check && cargo check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src tests docs AGENTS.md
git commit -m "feat: scaffold package, domain, config, errors, docs skeleton"
```

---

### Task 2: SQLite migrations, repository, FTS5, lifecycle reconcile + purge

**Files:**
- Create: `src/storage/mod.rs`, `src/storage/sqlite.rs`, `src/storage/repository.rs`, `src/storage/fts.rs`, `src/storage/lifecycle.rs`
- Modify: `src/lib.rs`
- Test: `tests/storage_test.rs`

**Interfaces:**
- Consumes: `Experience, State, SearchQuery, AppError, LifecycleConfig` from Task 1.
- Produces:
  - `pub fn open_db(path: &Path) -> Result<Connection, AppError>` (applies migrations 1..=N; enables WAL; sets `busy_timeout=5000`).
  - `pub trait ExperienceRepo { fn insert(&self, e: &Experience) -> Result<(), AppError>; fn get(&self, topic: &str, id: &str) -> Result<Option<Experience>, AppError>; fn update(&self, e: &Experience) -> Result<(), AppError>; fn soft_delete(&self, topic: &str, id: &str, now: DateTime<Utc>) -> Result<bool, AppError>; fn clear(&self, cmd: &ClearCommand, now: DateTime<Utc>) -> Result<u64, AppError>; fn search_fts(&self, text: &str, topic: Option<&str>, deep: bool) -> Result<Vec<(Experience, f32)>, AppError>; fn browse(&self, topic: Option<&str>, deep: bool) -> Result<Vec<Experience>, AppError>; }`
  - `pub fn reconcile_before_op(conn: &Connection, now: DateTime<Utc>, cfg: &LifecycleConfig) -> Result<(), AppError>` (transitions + sets `retention_started_at` on new forgotten; then `purge_expired`)
  - `pub fn purge_expired(conn: &Connection, now: DateTime<Utc>, cfg: &LifecycleConfig) -> Result<u64, AppError>`
  - Re-exports: `pub use repository::SqliteRepo` (struct wrapping `Mutex<Connection>`, `Clone` via `Arc`).

- [ ] **Step 1: Write the failing test**

```rust
// tests/storage_test.rs — use tempdir file DB
#[test] fn crud_roundtrip_unix_epoch() { /* insert + get: updated_at second-precision preserved */ }
#[test] fn compound_key_unique() { /* second insert same (topic,id) => Err(Storage(_)) */ }
#[test] fn fts_bm25_and_topic_filter() { /* insert 2, search_fts("trigger") returns BM25 order; topic filter narrows */ }
#[test] fn fts_invalid_syntax_is_typed() { /* search_fts("NEAR(broken",..) == Err(InvalidFtsSyntax(_)) */ }
#[test] fn lifecycle_reconcile_and_purge() { /* 61d-old active=>inactive; 121d-old=>forgotten w/ retention_started_at; forgotten+61d-retention purged with FTS rows */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test storage_test`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement migrations + `SqliteRepo` + reconcile/purge**

Schema: `experiences(topic TEXT, id TEXT, when_text TEXT NOT NULL, if_text TEXT NOT NULL, do_text TEXT NOT NULL, check_text TEXT NOT NULL, updated_at INTEGER NOT NULL, good_count INTEGER NOT NULL CHECK(>=0), bad_count INTEGER NOT NULL CHECK(>=0), state TEXT NOT NULL CHECK(state IN ('active','inactive','deleted','forgotten')), retention_started_at INTEGER, PRIMARY KEY(topic,id))`; `experiences_fts` as `FTS5(topic, when_text, if_text, do_text, check_text, content='experiences', content_rowid='rowid')` + insert/update/delete triggers + `rebuild_fts()` helper. Comparison uses strict `>` on epoch seconds: `(now - updated_at) > period_days*86400`. All reconcile+purge in one `transaction()`. Map rusqlite FTS syntax errors (message contains `syntax error` / `fts5`) to `InvalidFtsSyntax`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test storage_test && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/storage tests/storage_test.rs src/lib.rs
git commit -m "feat: sqlite storage, FTS5, lifecycle reconcile and purge"
```

---

### Task 3: MemoryService core (CRUD + feedback + lifecycle + FTS/browse search)

**Files:**
- Create: `src/application/mod.rs`, `src/application/service.rs`
- Modify: `src/lib.rs`
- Test: `tests/application_test.rs`

**Interfaces:**
- Consumes: `ExperienceRepo, reconcile_before_op, domain types, AppError, Config` from Tasks 1–2.
- Produces:
  - `pub struct MemoryService<R: ExperienceRepo + Send + Sync> { repo: R, lifecycle: LifecycleConfig }`
  - `impl<R> MemoryService<R> { pub fn new(repo: R, lifecycle: LifecycleConfig) -> Self; pub fn add(&self, cmd: AddCommand) -> Result<Experience, AppError>; pub fn modify(&self, cmd: ModifyCommand) -> Result<Experience, AppError>; pub fn delete(&self, topic: &str, id: &str) -> Result<(), AppError>; pub fn promote(&self, f: &FeedbackCommand) -> Result<Experience, AppError>; pub fn downgrade(&self, f: &FeedbackCommand) -> Result<Experience, AppError>; pub fn clear(&self, cmd: &ClearCommand) -> Result<u64, AppError>; pub fn search(&self, q: &SearchQuery) -> Result<Vec<Experience>, AppError>; }`
  - Search contract (this task): every method calls `reconcile_before_op` first; `add` validates + uuid id; `modify` rejects empty update + NotFound on deleted/missing + restores inactive/forgotten→active with cleared retention; `delete` idempotent on already-deleted; text-only search delegates to `search_fts` then clamps `limit=min(limit,100)`; neither-text-nor-semantic → `browse` ordered `updated_at DESC` + paginate. (`semantic`/RRF path added in Task 4; this task returns `EmbeddingUnavailable` if `semantic.is_some()`.)

- [ ] **Step 1: Write the failing test**

```rust
// tests/application_test.rs — real SqliteRepo on temp DB
#[test] fn add_modify_promote_delete_flow() { /* add good=1; modify restores forgotten→active; promote good=2; delete then modify => NotFound */ }
#[test] fn search_visibility_and_pagination() { /* deleted/forgotten excluded even deep; inactive only deep; limit=0=>[]; limit>100 clamps; offset beyond=>[] */ }
#[test] fn semantic_without_provider_is_typed() { /* search with semantic => Err(EmbeddingUnavailable(_)) */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test application_test`
Expected: FAIL.

- [ ] **Step 3: Implement `MemoryService` per signatures above**

Use `uuid::Uuid::new_v4().to_string()[..8]` for id (8-char prefix, retry on PK collision once). `updated_at = Utc::now()`. No embedding calls in this task.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test application_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/application tests/application_test.rs src/lib.rs
git commit -m "feat: MemoryService core with lifecycle and FTS search"
```

---

### Task 4: Embedding provider, vector store, semantic search + RRF

**Files:**
- Create: `src/embedding/mod.rs`, `src/embedding/provider.rs`, `src/embedding/openai.rs`, `src/embedding/fake.rs`
- Create: `src/storage/embeddings.rs`, `src/application/ranking.rs`
- Modify: `src/application/service.rs` (wire semantic path), `src/lib.rs`
- Test: extend `tests/application_test.rs` (new `#[test]` fns in same file — no new test file needed)

**Interfaces:**
- Consumes: `MemoryService`, `ExperienceRepo` from Tasks 2–3.
- Produces:
  - `#[async_trait] pub trait EmbeddingProvider: Send + Sync { async fn embed(&self, text: &str) -> Result<Vec<f32>, AppError>; fn model_id(&self) -> &str; }`
  - `pub struct OpenAiEmbeddingProvider { endpoint: String, model: String, api_key: String, timeout: Duration }` + `::new(cfg: &EmbeddingConfig) -> Self`
  - `pub struct FakeEmbeddingProvider { dim: usize }` (deterministic hash-word → unit vector; for tests)
  - `pub trait VectorStore { fn upsert_vector(&self, topic: &str, id: &str, model: &str, v: &[f32]) -> Result<(), AppError>; fn load_vectors(&self, topic: Option<&str>, model: &str) -> Result<Vec<(String, String, Vec<f32>)>, AppError>; fn delete_vectors(&self, topic: &str, id: &str) -> Result<(), AppError>; }` (implemented for `SqliteRepo`; `embeddings(topic TEXT, id TEXT, model TEXT, dims INTEGER, vec BLOB, PRIMARY KEY(topic,id,model))` with FK cascade delete)
  - `pub fn cosine(a: &[f32], b: &[f32]) -> f32`, `pub fn rrf_fuse(text: &[(String,String)], sem: &[(String,String)], k: u32) -> Vec<((String,String), f32)>`
  - Extended `MemoryService<R: ExperienceRepo + VectorStore>`: `with_embedding_provider(P)`, full `search()` — semantic-only = cosine over eligible + tie-break `updated_at DESC, topic ASC, id ASC`; both = RRF `k=60` fused then tie-break; combined with embed failure => `Err(EmbeddingUnavailable)` (no fallback); `add`/`modify` best-effort embed (log + skip on failure, canonical write stands).

- [ ] **Step 1: Write the failing test**

```rust
// append to tests/application_test.rs
#[test] fn semantic_ranks_by_cosine() { /* fake provider; 3 records; semantic query returns expected order */ }
#[test] fn rrf_is_deterministic() { /* text+semantic both set; fused order stable across runs; tie-break by updated_at */ }
#[test] fn combined_search_embed_failure_is_typed() { /* provider returning Err => search Err(EmbeddingUnavailable), not text-only */ }
#[test] fn add_succeeds_when_embed_fails() { /* failing provider; add still Ok + record retrievable */ }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test application_test semantic_ ; cargo test --test application_test rrf_ ; cargo test --test application_test combined_ ; cargo test --test application_test add_succeeds_when_embed`
Expected: FAIL (no provider / no semantic path).

- [ ] **Step 3: Implement provider trait, OpenAI client, `VectorStore`, ranking, service wiring**

OpenAI-compatible `POST {endpoint}/embeddings {model, input}` via reqwest with timeout; non-2xx => `EmbeddingUnavailable`. Fake: `dim=64`, token-hash buckets normalized.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test application_test && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/embedding src/storage/embeddings.rs src/application tests/application_test.rs
git commit -m "feat: embedding provider, vector store, semantic search with RRF"
```

---

### Task 5: CLI surface

**Files:**
- Create: `src/surfaces/mod.rs`, `src/surfaces/cli/mod.rs`, `src/surfaces/cli/commands.rs`, `src/surfaces/cli/render.rs`
- Modify: `src/main.rs` (real dispatch + `serve`), `src/lib.rs`
- Test: `tests/cli_test.rs`

**Interfaces:**
- Consumes: `MemoryService<SqliteRepo>`, `Config` from Tasks 1–4.
- Produces:
  - clap `Cli { db: Option<PathBuf>, bind: Option<SocketAddr>, json: bool, command: Commands }`, `Commands::{Add{topic,when,if_,do_,check}, Search{topic,text,semantic,limit,offset,deep}, Modify{topic,id,when,if_,do_,check}, Delete{topic,id,yes}, Promote{topic,id}, Downgrade{topic,id}, Clear{topic,all,yes}, Serve{}}`
  - `pub fn run_cli<S: ExperienceRepo + VectorStore>(svc: &MemoryService<S>, cmd: &Commands, json: bool) -> Result<i32, AppError>`; exit codes 0 ok / 1 not-found / 2 invalid / 3 embedding-unavailable / 4 internal; stderr structured `{"error":...}` when `--json`.
  - Destructive rule: `delete`/`clear` require `--yes` or interactive `y/N` prompt; `clear` requires exactly one of `--topic`/`--all`.

- [ ] **Step 1: Write the failing test**

```rust
// tests/cli_test.rs — assert_cmd on built binary with temp DB
#[test] fn add_search_json_roundtrip() { /* add --json => parses to Experience with id; search --json lists it */ }
#[test] fn destructive_requires_confirmation() { /* delete without --yes + stdin "n" => non-zero, record still present */ }
#[test] fn clear_requires_topic_or_all() { /* clear with neither => exit 2 */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test cli_test`
Expected: FAIL.

- [ ] **Step 3: Implement clap commands + `run_cli` + `main.rs` dispatch**

`--if`/`--do` map to `if_`/`do_` fields. Human output: one-line summary + detail; JSON: serde of `Experience`/`Vec<Experience>`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test cli_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces src/main.rs tests/cli_test.rs
git commit -m "feat: CLI surface with JSON mode and confirmations"
```

---

### Task 6: Server runtime + REST API

**Files:**
- Create: `src/surfaces/http/mod.rs`, `src/surfaces/http/server.rs`, `src/surfaces/http/rest.rs`
- Modify: `src/main.rs` (`serve`), `src/lib.rs`
- Test: `tests/rest_test.rs`

**Interfaces:**
- Consumes: `MemoryService` from Tasks 3–4.
- Produces:
  - `pub async fn serve<R>(svc: MemoryService<R>, bind: SocketAddr) -> Result<(), AppError>` hosting all three surfaces on one `axum::Router`.
  - Routes exactly: `GET /healthz` (checks `SELECT 1` + migrations table present); `GET/POST /api/v1/experiences`; `PATCH/DELETE /api/v1/experiences/{topic}/{id}` (URL-decoded); `POST .../promote`, `POST .../downgrade`; `DELETE /api/v1/experiences?topic=X&confirm=true | ?all=true&confirm=true`.
  - `ApiError` → status mapping: InvalidInput/InvalidFtsSyntax→400, NotFound→404, EmbeddingUnavailable→503, other→500, JSON body `{"error": "..."}`.

- [ ] **Step 1: Write the failing test**

```rust
// tests/rest_test.rs — axum TestClient / real spawned server on 127.0.0.1:0
#[test] fn healthz_and_crud() { /* /healthz 200; POST add 201; GET search finds it; PATCH; POST promote; DELETE 204; GET after => 404 */ }
#[test] fn error_mapping() { /* invalid add => 400; missing => 404; clear without confirm=true => 400 */ }
#[test] fn pagination_clamped() { /* limit=10000 behaves as 100 */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test rest_test`
Expected: FAIL.

- [ ] **Step 3: Implement `server.rs` + `rest.rs` DTOs/handlers**

DTOs use README names (`when/if/do/check`); state shared via `Arc<MemoryService<SqliteRepo>>`. This task ships buildable skeletons for web (`/`) and MCP (`/mcp`) routes too (placeholder 501 with final paths), completed in Tasks 7–8.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test rest_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/http tests/rest_test.rs src/main.rs
git commit -m "feat: HTTP server runtime and REST API"
```

---

### Task 7: Server-rendered web UI

**Files:**
- Create: `src/surfaces/http/web.rs`, `templates/{base,search,detail,add,edit,clear}.html`
- Modify: `src/surfaces/http/server.rs` (mount real handlers)
- Test: `tests/web_test.rs`

**Interfaces:**
- Consumes: router from Task 6, `MemoryService` from Tasks 3–4.
- Produces:
  - `GET /` (search/browse + deep toggle), `GET /experiences/new`, `POST /experiences`, `GET /experiences/{topic}/{id}`, `GET /experiences/{topic}/{id}/edit`, `POST /experiences/{topic}/{id}`, `POST /experiences/{topic}/{id}/promote`, `POST .../downgrade`, `POST .../delete` (confirm field), `GET/POST /clear`. All call `MemoryService` directly (never REST). Askama auto-escaping ON; destructive POSTs require `confirm=yes` field.

- [ ] **Step 1: Write the failing test**

```rust
// tests/web_test.rs
#[test] fn pages_render_and_escape() { /* add record with <script>; GET / shows &lt;script&gt;; detail page contains topic/id */ }
#[test] fn forms_validate_and_confirm() { /* POST add missing check => 400 page; POST delete without confirm=yes => 400, record remains */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test web_test`
Expected: FAIL.

- [ ] **Step 3: Implement handlers + askama templates**

Minimal CSS inline, no JS. Reuse search/browse + pagination (`?limit&offset&deep`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test web_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/http/web.rs templates tests/web_test.rs
git commit -m "feat: server-rendered web UI"
```

---

### Task 8: HTTP MCP surface

**Files:**
- Create: `src/surfaces/http/mcp.rs`
- Modify: `src/surfaces/http/server.rs`
- Test: `tests/mcp_test.rs`

**Interfaces:**
- Consumes: router from Task 6, `MemoryService` from Tasks 3–4.
- Produces:
  - `POST/GET /mcp` streamable HTTP via `rmcp`; tools `search, add, modify, delete, promote, downgrade, clear` with schemas mirroring `SearchQuery/AddCommand/ModifyCommand/FeedbackCommand/ClearCommand` exactly; typed errors propagated as MCP errors (invalid→Invalid params, not-found→not found, embedding→internal with 503-equivalent message).

- [ ] **Step 1: Write the failing test**

```rust
// tests/mcp_test.rs
#[test] fn tool_list_has_seven_tools() { /* initialize + tools/list => 7 names exact */ }
#[test] fn add_then_search_via_tools() { /* tools/call add => id; tools/call search => found */ }
#[test] fn typed_errors_propagate() { /* tools/call modify on missing => error containing "not found" */ }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test mcp_test`
Expected: FAIL.

- [ ] **Step 3: Implement `mcp.rs` handlers (transport separate from tool logic)**

Tool handlers call the same `MemoryService` methods as CLI/REST; no MCP-specific rules.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test mcp_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/surfaces/http/mcp.rs tests/mcp_test.rs
git commit -m "feat: HTTP MCP tools surface"
```

---

### Task 9: Full verification, docs reconciliation, smoke test

**Files:**
- Modify: all `docs/*.md`, `AGENTS.md` (reconcile with actual behavior)
- Test: manual smoke (no new test file; full suite must pass)

**Interfaces:**
- Consumes: everything from Tasks 1–8.
- Produces: docs matching implementation; clean `fmt/check/test/clippy`; smoke log in `docs/TODO.md` (checked boxes + follow-ups).

- [ ] **Step 1: Write the verification checklist (as TODO update)**

Add to `docs/TODO.md`: `[ ]` items for fmt, check, test, clippy, CLI smoke, REST smoke, web smoke, MCP smoke, docs review.

- [ ] **Step 2: Run full suite to verify current state**

Run: `cargo fmt --check; cargo check; cargo test; cargo clippy --all-targets --all-features -- -D warnings`
Expected: fix whatever fails in Step 3.

- [ ] **Step 3: Reconcile docs + fix findings**

Update `docs/cli.md` (actual flags/exit codes), `rest-api.md` (actual schemas/statuses), `mcp.md` (actual schemas), `storage.md` (actual schema/migrations), `architecture.md` + `web-server.md` + `development.md` as needed. Keep `AGENTS.md` invariants accurate.

- [ ] **Step 4: Run smoke + suite to verify it passes**

Run: temp-DB CLI `add/search/promote/delete`; `serve` then curl `/healthz`, REST CRUD, `/` web page, `/mcp` tool list; then full `cargo test`.
Expected: all green; check off TODO boxes.

- [ ] **Step 5: Commit**

```bash
git add docs AGENTS.md
git commit -m "docs: reconcile docs with implementation, verification green"
```

---

## Self-Review

1. **Spec coverage:** domain/lifecycle (T1–T2), FTS/BM25 + browse + pagination (T2–T3), semantic/cosine/RRF/fake/failure policy (T4), all 7 ops + serve (T3–T6), CLI/REST/web/MCP contracts (T5–T8), errors/status codes (T1,T6), docs + AGENTS (T1,T9). Gaps: none.
2. **Step scan:** each test step names exact tests + assertions; each implement step names exact signatures/files; no step asks for two independent deliverables.
3. **Type consistency:** `ExperienceRepo`/`VectorStore`/`EmbeddingProvider`/`MemoryService` signatures identical across tasks; `ClearCommand::Topic/All` used by storage, service, CLI, REST, MCP alike; column names `when_text…` vs wire `when…` preserved everywhere.
4. **Review Focus:** all 5 items pinned to owning tasks (T2 FTS + boundary, T9→T7 XSS per renumber — see fix below, T3/T6 pagination, T4 timeout).
5. **Proportion:** 9 tasks for a ~3k-line first release; signatures + test names only, no bodies transcribed.

**Fix applied inline:** Review Focus item 3 referenced Task 9 for XSS; correct owner is Task 7 (web). List above already reflects Task 7.
