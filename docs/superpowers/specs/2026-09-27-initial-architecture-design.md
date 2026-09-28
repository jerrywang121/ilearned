# ilearned Initial Architecture

- **Date:** 2026-09-27
- **Status:** Approved design
- **Scope:** Initial Rust implementation and first-release documentation

## Context and goals

ilearned is a local-first memory-management tool for AI agents. Its records are
learned experiences rather than plain facts: each experience describes when it
applies, its trigger, the action to try, and the signal that indicates success.
The agent can search, revise, and provide feedback on this living rule book.

The first release will:

- implement the experience domain and lifecycle rules from `README.md`;
- persist canonical records in SQLite;
- provide FTS5/BM25 text search in every installation;
- provide optional embedding-backed semantic search through an
  OpenAI-compatible API;
- expose one shared application service through CLI, REST, a minimal
  server-rendered web UI, and HTTP MCP;
- scaffold all four surfaces in the first milestone while keeping behavior in
  the shared core;
- run as one local single-user Rust binary with no accounts or authentication.

## Explicit non-goals

The first release will not add multi-user ownership, accounts, authentication,
separate service processes, a mandatory embedding provider, a bundled local
embedding model, or a JavaScript frontend build pipeline. It will not require a
SQLite vector extension: the initial semantic implementation will use a
replaceable vector-store interface backed by SQLite and application-side cosine
scoring.

## Architecture

ilearned is a modular monolith. One executable owns configuration, database
initialization, migrations, and all surface adapters. The dependency direction
is:

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

The source layout will keep these responsibilities visible:

- `domain`: `Experience`, lifecycle state, validated commands, search input,
  result types, and rules that do not depend on a transport or database.
- `application`: the `MemoryService` implementation of `search`, `add`,
  `modify`, `delete`, `promote`, `downgrade`, and `clear`. This is the only
  entry point available to adapters.
- `storage`: SQLite connection setup, migrations, experience repositories,
  FTS5 queries, embedding persistence, lifecycle reconciliation, and purge
  transactions.
- `embedding`: an interface for optional embedding generation plus the
  OpenAI-compatible HTTP implementation and a deterministic test fake.
- `config` and runtime initialization: database path, bind address, lifecycle
  periods, retention, embedding settings, logging, and server startup.
- `surfaces/cli`: command parsing, human/JSON rendering, exit codes, and
  confirmation prompts.
- `surfaces/http`: REST routes, web routes/templates, MCP HTTP transport, and
  conversion between wire DTOs and application commands.

Adapters must not contain SQL, lifecycle transitions, ranking logic, or
surface-specific copies of the public operations. The CLI and server use the
same initialization path: load configuration, open and migrate SQLite, build
the repositories and `MemoryService`, then dispatch the selected mode.

## Runtime and configuration

The executable will provide these top-level modes:

- `add`, `search`, `modify`, `delete`, `promote`, `downgrade`, and `clear` for
  direct CLI operations;
- `serve` to start REST, web, and MCP on one listener.

Server mode will default to `127.0.0.1:8787`. The database path, bind address,
active period, forget period, forgotten/deleted retention period, and embedding
settings are configurable. Configuration precedence is command-line flags,
then environment variables, then local defaults. The default database is a
file named `ilearned.db` in the current working directory. A non-loopback bind
is an explicit deployment choice and does not imply that authentication exists.

The embedding configuration is optional. FTS5 remains usable when it is absent.
When configured, embedding generation is best effort during add/modify and
never rolls back the canonical experience. A requested semantic search reports
a typed provider/configuration error if it cannot obtain an embedding.

## Domain model and lifecycle

The public experience shape is:

| Field | Meaning |
| --- | --- |
| `topic` | Grouping key |
| `id` | Opaque identifier unique within `topic`, generated on add |
| `when` | Scenario where the experience applies |
| `if` | Trigger or observation that activates it |
| `do` | Action the agent should try |
| `check` | Signal used to evaluate whether it helped |
| `updated_at` | Last explicit add/modify/feedback/delete/clear timestamp |
| `good_count` | Positive application count |
| `bad_count` | Negative application count |
| `state` | `active`, `inactive`, `deleted`, or `forgotten` |

SQLite will use `when_text`, `if_text`, `do_text`, and `check_text` column names
to avoid SQL keyword conflicts; transport and domain serialization uses the
README names. `updated_at` and `retention_started_at` are stored as INTEGER
Unix epoch seconds in UTC and serialized as RFC 3339 (ISO 8601) strings in
JSON and HTML. The canonical table has a compound primary key `(topic, id)`.
An internal nullable `retention_started_at` column records when an experience
entered `deleted` or `forgotten`; it is not exposed as part of the public
experience shape and prevents an old `updated_at` from causing an immediately
forgotten record to be purged.

The state rules are:

1. `add` generates an id, requires `topic`, `when`, `if`, `do`, and `check`,
   sets `state=active`, `good_count=1`, `bad_count=0`, and sets `updated_at`.
2. `modify` requires at least one of `when`, `if`, `do`, `check` to be
   supplied; a request with none of them is invalid input. `modify`, `promote`,
   and `downgrade` refresh `updated_at`, clear retention metadata, and restore
   an `inactive` or `forgotten` record to `active`. Records in `deleted` state
   are treated as not found by `modify`, `promote`, `downgrade`, and search; a
   `delete` on an already-`deleted` record is idempotent and succeeds without
   changing state. `forgotten` records never appear in search results, even
   with `deep=true`, but remain reachable by explicit `(topic, id)` lookup for
   `modify`, `promote`, `downgrade`, and `delete` until physically purged.
3. `promote` increments `good_count`; `downgrade` increments `bad_count`.
4. `delete` and `clear` mark records `deleted`, refresh `updated_at`, and set
   `retention_started_at` rather than removing data immediately.
5. Lifecycle reconciliation runs before every application operation. A
   non-deleted record older than the active period becomes `inactive`; one
   older than the forget period becomes `forgotten` and receives a retention
   start timestamp. The defaults are 60 days and 120 days, respectively.
6. Deleted or forgotten records are physically purged once their retention
   start is older than the configurable retention period, whose default is 60
   days. Purging also removes their FTS and embedding rows.

Lifecycle reconciliation and the requested operation are coordinated in the
same transaction where consistency requires it. Inactive state is not a
permanent tombstone: `modify`, `promote`, or `downgrade` can make the record
active again and refresh its timestamp.

## SQLite storage and search

Migrations will create:

- the canonical experiences table with validation constraints for state and
  non-negative counters;
- an FTS5 table covering `topic`, `when_text`, `if_text`, `do_text`, and
  `check_text`, with synchronization triggers and a rebuild path;
- an embeddings table keyed by `(topic, id, model)`, storing dimensions and a
  serialized vector for the configured embedding model.

Text search passes supported FTS5 match syntax to SQLite and uses BM25 rank.
The exact topic filter is applied outside the FTS expression, and lifecycle
eligibility is always applied after the text match. A search with neither
`text` nor `semantic` is a paginated browse ordered by `updated_at DESC`.
Default pagination is `limit=20`, `offset=0`; the implementation will enforce
a documented maximum limit.

Semantic search embeds the natural-language query, scores eligible stored
vectors by cosine similarity, and omits experiences that do not yet have a
vector. If both `text` and `semantic` are supplied and the embedding call
fails, the request fails with a typed provider/configuration error rather than
silently degrading to text-only search. When both queries succeed, the
application fuses the two ranked lists using reciprocal-rank fusion with
constant `k=60`:

```text
score(record) = sum(1 / (60 + rank))
```

Results sort by fused score descending with deterministic tie-breakers of
`updated_at DESC`, `topic ASC`, and `id ASC`. Deleted and forgotten records are
never eligible. Inactive records are eligible only when `deep=true`.

## Surface contracts

### CLI

The commands correspond directly to the public operations:

```text
ilearned add --topic TOPIC --when TEXT --if TEXT --do TEXT --check TEXT
ilearned search [--topic TOPIC] [--text MATCH] [--semantic QUERY]
                 [--limit N] [--offset N] [--deep]
ilearned modify --topic TOPIC --id ID [--when TEXT] [--if TEXT]
                 [--do TEXT] [--check TEXT]
ilearned delete --topic TOPIC --id ID
ilearned promote --topic TOPIC --id ID
ilearned downgrade --topic TOPIC --id ID
ilearned clear (--topic TOPIC | --all)
ilearned serve
```

The CLI has one global configuration selector, `--config-file PATH`, which
overlays the default global and local TOML files for that invocation. Runtime
settings otherwise come from environment variables and config files; the
`serve --bind ADDR` subcommand option is the highest-precedence bind override.
The config selector and all runtime settings are CLI-only: REST, web, and MCP
surfaces do not expose configuration controls. A `--json` output mode is
available on individual CLI commands, not as a global option. Human output is
intended for interactive use; JSON output is stable for agent and script use.
Destructive delete/clear commands require an explicit confirmation, and CLI
failures write a structured error to stderr and return a non-zero exit code.

### REST

The versioned JSON API will use these routes:

| Method | Route | Operation |
| --- | --- | --- |
| `GET` | `/healthz` | Readiness check; verifies SQLite is reachable and migrations are applied |
| `GET` | `/api/v1/experiences` | Search or browse with query parameters |
| `POST` | `/api/v1/experiences` | Add |
| `PATCH` | `/api/v1/experiences/{topic}/{id}` | Modify selected fields |
| `DELETE` | `/api/v1/experiences/{topic}/{id}` | Delete one |
| `POST` | `/api/v1/experiences/{topic}/{id}/promote` | Positive feedback |
| `POST` | `/api/v1/experiences/{topic}/{id}/downgrade` | Negative feedback |
| `DELETE` | `/api/v1/experiences` | Clear by topic or all with an explicit confirmation parameter |
| `POST`/`GET` | `/mcp` | HTTP MCP transport and tool dispatch |

REST query and request bodies use the public field names and expose typed
errors. Validation maps to 400, missing records (including `deleted` records,
which are treated as not found) map to 404, embedding provider/configuration
failures required by the request map to 503, and unexpected storage or runtime
failures map to 500. The API has no authentication in this local-first
release.

### Web

The web UI will use server-rendered HTML and the same listener as REST. It will
provide a search/browse page, add form, edit form, experience detail page,
feedback actions, delete action, and clear-by-topic/all flow. Destructive
actions require an explicit confirmation. Handlers call `MemoryService`
directly and do not call REST internally; HTML escaping and form validation are
mandatory.

### HTTP MCP

The `/mcp` adapter will expose tools named `search`, `add`, `modify`, `delete`,
`promote`, `downgrade`, and `clear`. Tool schemas and result semantics match
the application commands exactly. The adapter will implement the current
HTTP/streamable MCP transport required by the selected Rust MCP integration,
while keeping transport code separate from the tool handlers. No MCP-specific
business rules or alternate record shape will be introduced.

## Error and transaction policy

The domain/application layer will use typed errors that distinguish invalid
commands, missing records, unavailable embedding services, invalid FTS
syntax, and storage/runtime failures. SQLite mutations that change a
record, its FTS entry, its embedding metadata, or lifecycle state will be
transactional. An optional embedding failure is logged and represented as a
missing vector; it does not invalidate the canonical write. A semantic query
cannot silently fall back to unrelated text search.

## Testing and acceptance

The implementation is complete only when the following are covered:

- domain tests for required fields, generated identity, counter changes,
  timestamps, state transitions, retention, and search visibility;
- SQLite integration tests for migrations, compound identity, constraints,
  FTS synchronization/rebuild, BM25 queries, pagination, lifecycle purge, and
  rollback;
- application tests for all seven operations, deterministic RRF, fake-provider
  success, missing-provider behavior, and provider failure during writes;
- CLI tests for command parsing, JSON output, confirmation, and exit codes;
- REST tests for route schemas, status/error mapping, pagination, and clear
  confirmation;
- MCP tests for tool names, schemas, dispatch, and typed errors;
- web handler tests for escaped rendering, form validation, and destructive
  action confirmation;
- a manual documentation review pass before each release, verifying that
  public behavior, commands, routes, and schemas in `docs/` match the
  implementation; this is a human checklist item, not an automated test.

Once the Cargo scaffold exists, the authoritative local checks will be
`cargo fmt --check`, `cargo check`, `cargo test`, and
`cargo clippy --all-targets --all-features -- -D warnings`. The first milestone
must include buildable route/command/tool skeletons for all surfaces even when
later milestones fill in optional embedding behavior.

## Documentation and agent guidance

The maintained project documentation will include:

- `docs/architecture.md` — this architecture in user-facing form;
- `docs/TODO.md` — ordered implementation milestones and explicit follow-ups;
- `docs/cli.md` — commands, flags, output, confirmation, and exit codes;
- `docs/web-server.md` — runtime modes, bind/configuration, web routes, and
  co-hosted REST/MCP surfaces;
- `docs/rest-api.md` — versioned JSON routes, schemas, pagination, and errors;
- `docs/mcp.md` — tool contracts and HTTP transport;
- `docs/storage.md` — schema, migrations, FTS5, embeddings, lifecycle, and
  retention;
- `docs/development.md` — setup and verification commands.

Root `AGENTS.md` will direct future agents to the README and these documents,
record the shared application-service boundary, list lifecycle/search
invariants, identify the verification commands, and require documentation and
tests to be updated with behavior changes.

## Implementation sequence

1. Create the Cargo package, configuration/error foundations, domain types, and
   the complete documentation/agent-guidance skeleton.
2. Add SQLite migrations, repositories, lifecycle reconciliation, FTS5 search,
   and storage integration tests.
3. Add `MemoryService` and complete the CLI operations and tests.
4. Add the server runtime and build REST, server-rendered web, and HTTP MCP
   skeletons in the same milestone, then complete their contracts and tests.
5. Add the optional embedding provider, vector persistence/cosine search, RRF,
   and provider failure behavior.
6. Run the full verification suite, reconcile docs with actual behavior, and
   perform an end-to-end local smoke test across CLI, REST, web, and MCP.
