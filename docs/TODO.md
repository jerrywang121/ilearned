# TODO

Ordered implementation milestones (see `docs/superpowers/plans/2026-09-27-initial-architecture.md`).

- [x] Task 1: Scaffold, domain, config, errors, docs skeleton
- [x] Task 2: SQLite migrations, repository, FTS5, lifecycle reconcile + purge
- [x] Task 3: MemoryService core (CRUD + feedback + lifecycle + FTS/browse search)
- [x] Task 4: Embedding provider, vector store, semantic search + RRF
- [x] Task 5: CLI surface
- [x] Task 6: Server runtime + REST API
- [x] Task 7: Server-rendered web UI
- [x] Task 8: HTTP MCP surface
- [x] Task 9: Full verification, docs reconciliation, smoke test
  - [x] `cargo fmt --check` green
  - [x] `cargo check` green
  - [x] `cargo test` (full suite) green
  - [x] `cargo clippy --all-targets --all-features -- -D warnings` green
  - [x] CLI smoke on temp DB: add → search → promote → delete
  - [x] REST smoke: `/healthz` + CRUD + error codes
  - [x] Web smoke: `/` renders, confirm-gated delete enforced
  - [x] MCP smoke: `tools/list` shows 7 tools
  - [x] Docs review: `cli.md`, `rest-api.md`, `mcp.md`, `storage.md`,
    `architecture.md`, `web-server.md`, `development.md`, `AGENTS.md`

## Follow-ups (post first-release)

- Documented maximum search limit tuning (`MAX_LIMIT`).
- Embedding model/dimension migration path.
- Backup/export story for the local SQLite file.
