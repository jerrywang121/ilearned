# TODO

Ordered implementation milestones (see `docs/superpowers/plans/2026-09-27-initial-architecture.md`).

- [ ] Task 1: Scaffold, domain, config, errors, docs skeleton
- [ ] Task 2: SQLite migrations, repository, FTS5, lifecycle reconcile + purge
- [ ] Task 3: MemoryService core (CRUD + feedback + lifecycle + FTS/browse search)
- [ ] Task 4: Embedding provider, vector store, semantic search + RRF
- [ ] Task 5: CLI surface
- [ ] Task 6: Server runtime + REST API
- [ ] Task 7: Server-rendered web UI
- [ ] Task 8: HTTP MCP surface
- [ ] Task 9: Full verification, docs reconciliation, smoke test

## Follow-ups (post first-release)

- Documented maximum search limit tuning (`MAX_LIMIT`).
- Embedding model/dimension migration path.
- Backup/export story for the local SQLite file.
