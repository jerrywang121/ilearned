# ilearned

> Lightweight, local-first memory management for AI agents — a live rule book of learned experiences, not plain facts.

[![Build](https://img.shields.io/badge/build-cargo%20test-green)](docs/development.md)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.98%2B-orange)](Cargo.toml)

## What it is

Agents learn by doing. `ilearned` stores those lessons as **experiences** — structured rules (when / if / do / check) grouped by topic — that the agent can search, apply, and continuously refine.

Each experience records:

| Field | Meaning |
| --- | --- |
| `topic` / `id` | Compound key; `id` is an 8-char unique id within the topic |
| `when` | Scenario this experience applies to |
| `if` | Trigger(s), e.g. something happened |
| `do` | Action(s) the agent should take / try |
| `check` | Signal(s) to verify the experience was useful |
| `updated_at` | Creation / last-update timestamp (RFC 3339 on the wire) |
| `good_count` / `bad_count` | Positive / negative feedback tallies |
| `state` | `active` \| `inactive` \| `deleted` \| `forgotten` (internal maintenance) |

All behavior lives in one application service (`MemoryService`); CLI, REST, web, and MCP surfaces are thin adapters over it.

## Features

- **One binary, four surfaces** — CLI subcommands plus `serve` mode hosting REST + server-rendered web UI + MCP (streamable HTTP) on a single listener (default `127.0.0.1:8787`).
- **Full-text + semantic search** — SQLite FTS5/BM25 always works; optional OpenAI-compatible embeddings add cosine search; combined queries fuse both with RRF (`k=60`).
- **Lifecycle management** — `active` (default 60d) → `inactive` → `forgotten` (default 120d), with configurable retention (default 60d) before physical purge. Search hides `deleted`/`forgotten` always, `inactive` unless `deep=true`.
- **Feedback loop** — `promote`/`downgrade` bump `good_count`/`bad_count` and restore records to active life.
- **Local-first** — single-user, SQLite (WAL) backend, no auth, no JS build.

## Installation

Requirements: Rust toolchain (1.98+) and a C toolchain (SQLite builds from source via rusqlite `bundled`; no OpenSSL headers needed — reqwest uses rustls).

```bash
git clone https://github.com/jerrywang121/ilearned
cd ilearned
cargo build --release
./target/release/ilearned --help
```

## Quickstart

```bash
# Add an experience
ilearned add --topic rust --when "deploy fails" --if "alert fires" \
  --do "restart worker" --check "health ok"

# Search it (human output by default, --json for agents/scripts)
ilearned --json search --topic rust --text deploy

# Positive feedback
ilearned promote --topic rust --id <id>

# Serve REST + web + MCP on one port
ilearned serve
```

## Usage

### CLI

```text
ilearned [--db PATH] [--bind ADDR] [--json] [--active-days N]
         [--forget-days N] [--retention-days N]
         [--embed-endpoint URL] [--embed-model NAME] [--embed-api-key KEY]
         [--embed-dims N] [--embed-timeout-secs N] <command>

ilearned add --topic TOPIC --when TEXT --if TEXT --do TEXT --check TEXT
ilearned search [--topic TOPIC] [--text MATCH] [--semantic QUERY]
                 [--limit N] [--offset N] [--deep]
ilearned modify --topic TOPIC --id ID [--when TEXT] [--if TEXT]
                 [--do TEXT] [--check TEXT]
ilearned delete --topic TOPIC --id ID [--yes]
ilearned promote --topic TOPIC --id ID
ilearned downgrade --topic TOPIC --id ID
ilearned clear (--topic TOPIC | --all) [--yes]
ilearned serve [--bind ADDR]
```

- Destructive `delete`/`clear` require `--yes` or an interactive `y/N` prompt (refusal aborts, exit 2). `clear` needs exactly one of `--topic` / `--all`.
- `modify` needs at least one non-blank field (blank-only values are ignored).
- Exit codes: `0` ok · `1` not-found · `2` invalid input · `3` embedding unavailable · `4` internal.

Full reference: [docs/cli.md](docs/cli.md).

### REST API

| Method | Route | Notes |
| --- | --- | --- |
| `GET` | `/healthz` | Readiness probe → `{"ok":true}` |
| `GET` / `POST` | `/api/v1/experiences` | Search/browse · add (201) |
| `PATCH` / `DELETE` | `/api/v1/experiences/:topic/:id` | Modify · delete (204; never-existing id → 404) |
| `POST` | `/api/v1/experiences/:topic/:id/promote` | `good_count + 1` |
| `POST` | `/api/v1/experiences/:topic/:id/downgrade` | `bad_count + 1` |
| `DELETE` | `/api/v1/experiences?topic=X&confirm=true` | Clear topic/all (exactly one of `topic` / `all=true`) |

Errors are `{"error": "..."}` with `400` validation · `404` missing · `503` embedding-unavailable · `500` internal. Pagination defaults `limit=20 offset=0`, clamped to `MAX_LIMIT=100`.

Full reference: [docs/rest-api.md](docs/rest-api.md).

### Web UI

Server-rendered HTML, no JavaScript: search/browse at `/`, record detail, add/edit forms, promote/downgrade/delete actions, and a clear flow. Destructive posts require `confirm=yes`; all output is HTML-escaped. See [docs/web-server.md](docs/web-server.md).

### MCP

Streamable HTTP at `/mcp` with seven tools mirroring the domain commands exactly: `search`, `add`, `modify`, `delete`, `promote`, `downgrade`, `clear` (requires `confirm=true`). Typed errors map to MCP errors (`invalid → invalid params`, `not-found → not found`, embedding failures → internal with message). See [docs/mcp.md](docs/mcp.md).

## Configuration

Precedence: **flags > `ILEARNED_*` env > defaults.**

| Setting | Flag | Env | Default |
| --- | --- | --- | --- |
| Database path | `--db` | `ILEARNED_DB` | `./ilearned.db` |
| Bind address | `--bind` | `ILEARNED_BIND` | `127.0.0.1:8787` |
| Active period (days) | `--active-days` | `ILEARNED_ACTIVE_DAYS` | `60` |
| Forget period (days) | `--forget-days` | `ILEARNED_FORGET_DAYS` | `120` |
| Retention (days) | `--retention-days` | `ILEARNED_RETENTION_DAYS` | `60` |
| Embedding endpoint/model/key | `--embed-endpoint/model/api-key` | `ILEARNED_EMBED_*` | unset (semantic search returns typed error) |
| Embedding dims/timeout | `--embed-dims/timeout-secs` | `ILEARNED_EMBED_*` | `1536` / `30s` |

Embedding failure on `add`/`modify` never rolls back the canonical write; a semantic query without a provider fails typed (exit 3 / HTTP 503) instead of silently degrading to text-only.

## Architecture

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

Details: [docs/architecture.md](docs/architecture.md) · [docs/storage.md](docs/storage.md) · [design spec](docs/superpowers/specs/2026-09-27-initial-architecture-design.md).

## Development

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

All four must pass before committing. See [docs/development.md](docs/development.md). Behavior changes require updating the matching `docs/*.md` and tests (see [AGENTS.md](AGENTS.md)).

## Roadmap

- Embedding model/dimension migration path
- Backup/export story for the local SQLite file
- Tunable `MAX_LIMIT` and richer web search filters

## License

MIT — see [LICENSE](LICENSE).
