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
  `update`, `delete`, `promote`, `demote`, `clear`, `list_topics`, and the
  CLI-only `migrate_embeddings` maintenance operation. The only entry point
  for adapters. Generic over `R: ExperienceRepo + VectorStore`; semantic
  search, cosine, RRF (`k=60`), and embedding migration rules live here. `add` retries only on
  primary-key/UNIQUE violations (other storage errors propagate); unknown
  DB `state` values surface as storage errors instead of defaulting.
- `storage` — shared key-aware SQLite/SQLCipher connection, migrations, repositories, FTS5 queries,
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
- Embedding failure on `add`/`update` never rolls back the canonical write.
- A semantic query that cannot obtain an embedding fails typed — never
  silently degrades to text-only search.
- Embeddings are identified by `(model, dims)` and semantic search loads only
  the exact query identity; mismatched dimensions are never truncated for
  cosine scoring.
- `embedding migrate` re-embeds every non-deleted experience, including
  inactive and forgotten records, stages new vectors without changing
  canonical records, and retains old identities unless a completely
  successful run uses confirmed `--prune`. It is a CLI-only maintenance
  operation; HTTP and MCP do not expose it.
- The shared storage opener receives the optional process-local
  `ILEARNED_DB_KEY`. With no key it preserves plaintext SQLite compatibility;
  with a key it applies SQLCipher's key before any schema access and enables
  `cipher_memory_security`, then performs the existing WAL, timeout, migration,
  and schema setup. The key is never stored in TOML or configuration output.
- Plaintext-to-encrypted conversion is explicit (`ilearned db encrypt`), runs
  before keyed service construction, and uses `sqlcipher_export` followed by
  SQLCipher integrity and schema verification. Only a verified destination is
  atomically substituted for the source; failed conversions leave the source
  usable and do not retain a plaintext backup. Stale plaintext `-wal`/`-shm`
  sidecars are removed after successful replacement. JSONL exports are outside
  this database-encryption boundary.

Full design: `docs/superpowers/specs/2026-09-27-initial-architecture-design.md`.
Implementation plan: `docs/superpowers/plans/2026-09-27-initial-architecture.md`.
