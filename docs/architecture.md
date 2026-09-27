# Architecture

ilearned is a modular monolith. One executable owns configuration, database
initialization, migrations, and all surface adapters.

```text
┌──────────────────────────────────────────────────┐
│ Adapters: CLI · REST · Web · HTTP MCP            │
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
- `application` — `MemoryService` implementing `search`, `add`, `modify`,
  `delete`, `promote`, `downgrade`, `clear`. The only entry point for adapters.
- `storage` — SQLite connection, migrations, repositories, FTS5 queries,
  embedding persistence, lifecycle reconciliation, purge transactions.
- `embedding` — optional embedding interface + OpenAI-compatible HTTP client.
- `surfaces/cli`, `surfaces/http` — thin adapters. No SQL, lifecycle
  transitions, or ranking logic.

## Key invariants

- One binary: CLI subcommands + `serve` mode; REST + web + MCP share one
  listener (default `127.0.0.1:8787`).
- Single-user local-first; no authentication.
- Search never returns `deleted`/`forgotten` (even with `deep=true`);
  `inactive` only when `deep=true`.
- Embedding failure on `add`/`modify` never rolls back the canonical write.
- A semantic query that cannot obtain an embedding fails typed — never
  silently degrades to text-only search.

Full design: `docs/superpowers/specs/2026-09-27-initial-architecture-design.md`.
Implementation plan: `docs/superpowers/plans/2026-09-27-initial-architecture.md`.
