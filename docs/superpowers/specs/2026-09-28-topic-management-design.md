# Topic Hierarchy, Wildcard Search & Topic Management

- **Date:** 2026-09-28
- **Status:** Approved design (user approved in-chat bounded→architectural upgrade; explicit "persist spec, then plan, then implement")
- **Scope:** Hierarchical topic validation, MQTT-style `#` wildcard on experience search/export, new topic list/search on all surfaces (CLI, REST, web, MCP)
- **Prior spec:** `docs/superpowers/specs/2026-09-27-initial-architecture-design.md`
- **Prior plan:** `docs/superpowers/plans/2026-09-27-initial-architecture.md`

## Context and goals

Experiences are grouped by `topic` (compound key `(topic, id)`). Topics are
currently free-form strings. The goal:

1. Enforce a hierarchical topic form, e.g. `travel/hotel/checkout`, with no
   special characters.
2. Update every `--topic` help text to document the form.
3. Support MQTT-style multi-level wildcard `#` on experience `search --topic`
   (and `--topic` on `export`): `travel/#`, `travel/#/checkout`, `#/checkout`.
4. Add a `topic` command group with `list` and `search` subcommands, paginated
   via `--limit`/`--offset`, on **all** surfaces.

Success: invalid topics rejected with typed errors on every write path;
wildcard search returns exactly the matching records on FTS + semantic +
browse paths; topic listing works identically on CLI/REST/web/MCP; docs and
tests updated per `AGENTS.md`.

## Explicit non-goals

- No `+` single-level wildcard (only `#`).
- No wildcard on destructive paths: `clear --topic` and `delete` stay
  exact-match (clear additionally rejects `#` as invalid input).
- No topic rename/move operation.
- No migration of existing data: old topics that violate the new form stay
  readable/searchable; only new writes (`add`, `import`, `clear --topic`
  target) are validated.
- No change to `MAX_LIMIT=100`, RRF `k=60`, lifecycle rules, or embedding
  behavior.

## Topic form (agreed: strict-lowercase)

Canonical topic (stored in SQLite, used as compound key):

- Non-empty string; segments split on `/`.
- Each segment: one or more chars from `[a-z0-9_-]` (lowercase ASCII
  letters, digits, hyphen, underscore). No empty segments.
- Rejected: leading/trailing `/`, `//`, uppercase, dots, spaces, any other
  special char, and `#` (hash is pattern-only, never stored).

Topic *pattern* (search/export `--topic` filter, topic-search query):

- Same rules, except a segment may be exactly `#` (full-segment only;
  partial like `trav#` is invalid).

Shared help text for every `--topic` / `topic` field on all surfaces:

> `hierarchical topic, e.g. travel/hotel/checkout; segments [a-z0-9_-], '/' separated; search accepts # multi-level wildcard`

## Domain: `src/domain/topics.rs` (new)

```rust
pub fn validate_topic(topic: &str) -> Result<(), AppError>;
pub fn validate_topic_pattern(pattern: &str) -> Result<(), AppError>;
pub fn topic_matches(pattern: &str, topic: &str) -> bool;
pub fn truncate_topic(topic: &str, level: u32) -> String;
```

- `validate_topic`: enforces the canonical form above; failure →
  `AppError::InvalidInput("invalid topic '...': ...")`.
- `validate_topic_pattern`: same, but allows `#` segments.
- `topic_matches(pattern, topic)`: recursive segment matcher where `#`
  matches **zero or more** levels (agreed multi-level MQTT semantics):
  - `travel/#` matches `travel`, `travel/hotel`, `travel/hotel/checkout`.
  - `#` alone matches everything.
  - `#/checkout` matches `checkout`, `travel/checkout`, `a/b/checkout`.
  - `travel/#/checkout` matches `travel/checkout`, `travel/hotel/checkout`.
  - Bare `travel` matches **only** exact `travel` (agreed exact-only;
    descendants require `travel/#`).
- `truncate_topic(topic, level)`: first `level` segments joined by `/`
  (used by `--level`).

`domain/mod.rs` re-exports the module.

## Application: `MemoryService`

Validation call sites:

- `add`: `validate_topic(&cmd.topic)?` before `cmd.validate()` semantics
  (kept inside `AddCommand::validate` path so all adapters share it).
- `import_record` / `validate_import`: `validate_topic(&e.topic)?`.
- `clear`: `ClearCommand::Topic(t)` → `validate_topic(t)?` (rejects `#`).
- `search` / `export` topic filter: `validate_topic_pattern(t)?`.
- Explicit `(topic, id)` reads (`get`, `modify`, `promote`, `downgrade`,
  `delete`): no validation — lookup miss (or deleted) reads as `NotFound`,
  preserving the sole-entry `get` contract.

Wildcard search execution:

- Pattern without `#` → keep the existing fast exact `topic=?` SQL path
  (`search_fts`, `browse`, `load_vectors`).
- Pattern with `#` → pass `None` as the SQL topic filter (superset fetch),
  then filter in Rust via `topic_matches` **before** the visibility filter
  and pagination, on all three paths (browse, FTS, semantic) and the
  combined RRF path. Same ordering/tie-breaks as today; `limit` clamped to
  `MAX_LIMIT`, `limit=0`/over-range offset → `[]`.

New method:

```rust
pub struct TopicQuery { pub query: Option<String>, pub level: Option<u32>,
    pub limit: u32, pub offset: u32, pub deep: bool }

pub fn list_topics(&self, q: &TopicQuery) -> Result<Vec<String>, AppError>
```

- Reconcile first (like every op).
- Distinct topics from repo honoring visibility: `deep=false` → only
  topics with ≥1 `active` record; `deep=true` → topics with ≥1
  `active`/`inactive` record. `deleted`/`forgotten` never contribute.
- `level`: `Some(0)` → `InvalidInput`; else truncate each topic, dedup.
- `query`: blank/`None` → list all. Contains `#` → pattern match
  (`validate_topic_pattern` + `topic_matches` against the **full** topic
  before truncation); else case-sensitive substring `contains`.
- Sort alphabetically, paginate (`limit.min(MAX_LIMIT)`, `offset`).

`commands.rs` gains `TopicQuery`; `application/mod.rs` re-exports it.

## Storage

`ExperienceRepo` gains:

```rust
fn distinct_topics(&self, deep: bool) -> Result<Vec<String>, AppError>;
```

`SqliteRepo` impl:

```sql
-- deep=false
SELECT DISTINCT topic FROM experiences WHERE state='active' ORDER BY topic;
-- deep=true
SELECT DISTINCT topic FROM experiences WHERE state IN ('active','inactive') ORDER BY topic;
```

No schema migration (topics are plain `TEXT`; form enforced in domain, not
with a `CHECK` constraint, so legacy rows stay readable). Service does
level/query/pagination in Rust over this list.

## Surface contracts

### CLI

```text
ilearned topic list [--level N] [--limit N] [--offset N] [--deep]
ilearned topic search QUERY [--level N] [--limit N] [--offset N] [--deep]
```

- `Commands::Topic(TopicArgs)` subcommand group; both subcommands share
  pagination flags (`limit` default 20, `offset` default 0, clamped
  service-side like search) and `--deep`.
- `search QUERY`: positional (or `--query`) string; plain substring or `#`
  pattern (validated service-side).
- `--topic` help on `add/search/modify/delete/promote/downgrade/clear/export`
  updated to the shared help text above; `search --topic` help appends
  `#` examples (`travel/#`, `#/checkout`).
- Human output: one topic per line; `--json`: JSON array of strings.
- `main.rs` dispatch extended to the new variant.

### REST

| Method | Route | Notes |
| --- | --- | --- |
| `GET` | `/api/v1/topics?level=&q=&limit=&offset=&deep=` | No `q` = list, with `q` = search; returns `200 ["travel/hotel", ...]` |

- `level=0` → 400; invalid pattern → 400; pagination identical to search
  (`limit` default 20, clamp 100).
- Topic strings with `/` need no special encoding in query params.

### Web

- `GET /topics?level=&q=&limit=&offset=&deep=`: server-rendered page with a
  level/q form + result list (Askama auto-escape ON, like existing pages).
- Index page (`/`) gains a link to `/topics`.
- Route registered in `web_routes`.

### MCP (both transports: streamable HTTP `/mcp` + stdio `mcp`)

Two new tools (schemas mirror `TopicQuery`):

- `topics_list{level?,limit?,offset?,deep?}`
- `topics_search{query,level?,limit?,offset?,deep?}`

Results: JSON array of strings. Error mapping unchanged
(`InvalidInput` → `invalid_params`, etc.). Tool count grows 7 → 9; docs
updated. The `ServerHandler` instructions string mentions topic
listing.

## Error and transaction policy

- Invalid topic/pattern/level → `AppError::InvalidInput` → CLI exit 2,
  REST 400, web 400 page, MCP `invalid_params`.
- No new error variants; no new transactions (single `SELECT DISTINCT`;
  reconcile runs first as usual).
- `export --topic` accepts the same wildcard as search (validated as
  pattern, filtered in Rust when `#` present).

## Testing and acceptance

- `domain`: table tests for `validate_topic` (valid: `travel`,
  `travel/hotel/checkout`, `a-b/c_d/e9`; invalid: ``, `/`, `travel/`,
  `/travel`, `a//b`, `Travel`, `a.b`, `a b`, `a#`, `trav#`), pattern
  validation (`#`, `travel/#`, `#/checkout`, `travel/#/checkout` valid),
  `topic_matches` matrix (incl. `#` zero-level match, middle-`#`, bare
  exact-only), `truncate_topic`.
- `application/service`: wildcard search across browse/FTS/semantic(fake
  provider) incl. middle-`#`; exact-only bare; `list_topics` level dedup,
  substring vs pattern query, pagination clamp, `deep` visibility,
  `level=0` error, invalid add/import/clear rejected.
- `storage`: `distinct_topics` visibility (`deleted`/`forgotten` excluded,
  `inactive` only with deep).
- Per-surface: CLI (`topic list/search` human+JSON, exit codes), REST
  (`GET /api/v1/topics` shapes/statuses), web (`/topics` renders + escapes
  `<script>` topic text), MCP (both tools dispatch + typed errors over
  stdio; HTTP covered by shared handler).
- Verification (all four, per `AGENTS.md`):
  `cargo fmt --check`, `cargo check`, `cargo test`,
  `cargo clippy --all-targets --all-features -- -D warnings`.

## Documentation to update (with the change, per AGENTS.md)

- `docs/cli.md` — topic form, wildcard examples, `topic` subcommands.
- `docs/rest-api.md` — `GET /api/v1/topics` row + semantics.
- `docs/web-server.md` — `/topics` route row.
- `docs/mcp.md` — 7 → 9 tools, new arg schemas.
- `README.md` — CLI usage block + REST table + MCP tool count.
- `docs/architecture.md` — topic-form invariant + `list_topics` entry.
- `AGENTS.md` — topic-form invariant line.
