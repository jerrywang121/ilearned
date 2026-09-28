# Architecture

ilearned is a modular monolith. One executable owns configuration, database
initialization, migrations, and all surface adapters.

```text
┌──────────────────────────────────────────────────┐
│ Adapters: CLI · REST · Web · HTTP MCP · stdio MCP│
└────────────────────────┬─────────────────────────┘
                         ▼
┌──────────────────────────────────────────────────┐
│ Application service (MemoryService)              │
└──────┬──────────────────────────────┬────────────┘
       ▼                              ▼
┌──────────────────┐        ┌──────────────────────┐
│ Domain           │        │ Ports                │
│ Experience       │◄───────│ ExperienceRepo       │
│ State, rules     │◄───────│ EmbeddingProvider    │
└──────────────────┘        └──────────┬───────────┘
                                       ▼
                            ┌──────────────────────┐
                            │ SQLite + optional    │
                            │ OpenAI-compatible    │
                            │ embeddings HTTP API  │
                            └──────────────────────┘
```

## Layers

- `domain` — `Experience`, lifecycle state, validated commands, search input.
  No transport or database dependencies.
- `application` — `MemoryService` implementing `get`, `search`, `add`,
  `modify`, `delete`, `promote`, `downgrade`, `clear`, `list_topics`. The only entry point
  for adapters. Generic over `R: ExperienceRepo + VectorStore`; semantic
  search, cosine, and RRF (`k=60`) live here. `add` retries only on
  primary-key/UNIQUE violations (other storage errors propagate); unknown
  DB `state` values surface as storage errors instead of defaulting.
- `storage` — SQLite connection, migrations, repositories, FTS5 queries,
  embedding persistence, lifecycle reconciliation, purge transactions.
- `embedding` — `EmbeddingProvider` trait + OpenAI-compatible HTTP client +
  deterministic fake (tests) and failing (error-path tests) providers.
- `surfaces/cli`, `surfaces/http` — thin adapters. No SQL, lifecycle
  transitions, or ranking logic. The stdio MCP server (`ilearned mcp`)
  reuses the same `IlearnedTools` type and service as HTTP MCP; only the
  transport differs (stdio instead of streamable HTTP).

## HTTP layer shape

- One listener serves three sub-routers merged after each takes state:
  REST + web share `Arc<MemoryService<SqliteRepo>>`; MCP builds a stateless
  `Router<()>` from the same `Arc` (axum `nest_service` cannot inject
  state, and rmcp macros require a non-generic tools impl, so MCP and the
  `serve` path are concrete over `SqliteRepo`).
- Web templates are inline Askama `#[template(source = ...)]` (no
  `templates/` directory); auto-escaping is on.
- `main.rs` builds the service from `Config` (the CLI-selected config-file
  overlay, default config files, and environment) and attaches an
  OpenAI-compatible embedding provider when endpoint + model + API key are all
  configured; otherwise semantic search returns `EmbeddingUnavailable` on
  every surface. The only remaining runtime CLI override is `serve --bind`,
  which takes precedence over config files and environment. REST, web, and
  MCP are adapters over the resulting service and do not expose configuration
  controls.

## Key invariants

- One binary: CLI subcommands + `serve` mode; REST + web + MCP share one
  listener (default `127.0.0.1:8787`); `ilearned mcp` serves the same MCP
  tools over stdio. Database resolution uses configured paths first, then an
  existing project-local `./.ilearned/ilearned.db`, then an existing XDG data
  database; startup fails when none is configured or present.
- Single-user local-first; no authentication.
- Explicit `(topic, id)` reads go through `MemoryService::get` (sole entry):
  reconciles first, `deleted`/missing read as `NotFound`; `forgotten` and
  `inactive` remain reachable here (search still hides them).
- Topics are hierarchical (`travel/hotel/checkout`, segments `[a-z0-9_-]`,
  validated on write); `search`/`export --topic` accept `#` multi-level
  wildcards (bare topic = exact only, `clear --topic` stays exact);
  `MemoryService::list_topics` lists/searches distinct topics
  (query matched before `--level` truncation, paginated `limit/offset`).
- Search never returns `deleted`/`forgotten` (even with `deep=true`);
  `inactive` only when `deep=true`.
- Embedding failure on `add`/`modify` never rolls back the canonical write.
- A semantic query that cannot obtain an embedding fails typed — never
  silently degrades to text-only search.

Full design: `docs/superpowers/specs/2026-09-27-initial-architecture-design.md`.
Implementation plan: `docs/superpowers/plans/2026-09-27-initial-architecture.md`.
