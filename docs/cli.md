# CLI

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
ilearned mcp
```

- Global flags: `--db PATH` (`ILEARNED_DB`, default `./ilearned.db`),
  `--bind ADDR` (`ILEARNED_BIND`, default `127.0.0.1:8787`, only used by
  `serve`), `--json` for stable JSON output. Human output is for
  interactive use; JSON output is stable for agent and script use.
- Lifecycle tuning: `--active-days` (`ILEARNED_ACTIVE_DAYS`, default 60),
  `--forget-days` (`ILEARNED_FORGET_DAYS`, default 120),
  `--retention-days` (`ILEARNED_RETENTION_DAYS`, default 60).
  Precedence: flags > env > defaults.
- Embedding provider (optional): `--embed-endpoint` (`ILEARNED_EMBED_ENDPOINT`),
  `--embed-model` (`ILEARNED_EMBED_MODEL`), `--embed-api-key`
  (`ILEARNED_EMBED_API_KEY`), `--embed-dims` (`ILEARNED_EMBED_DIMS`,
  default 1536), `--embed-timeout-secs` (`ILEARNED_EMBED_TIMEOUT_SECS`,
  default 30). When endpoint+model+key are all present, the binary builds
  an OpenAI-compatible provider and `search --semantic` works; otherwise
  semantic queries exit 3.
- `add` prints the created `Experience` (JSON: the record; human: one-line
  summary plus fields). `search` prints a JSON array in `--json` mode.
- `modify` requires at least one non-blank field of
  `--when/--if/--do/--check` (blank-only values are ignored, so all-blank
  is rejected).
- Destructive `delete`/`clear` require explicit confirmation: `--yes` or an
  interactive `y/N` prompt on stderr. Refusal (including EOF on stdin
  without `--yes`) aborts with exit code 2 and changes nothing.
- `clear` requires exactly one of `--topic` / `--all` (enforced by clap;
  violations exit 2).
- `serve` starts REST + web + MCP on one listener. `--bind` on the `serve`
  subcommand wins over the global `--bind`.
- `mcp` runs the same 7 tools as an MCP server over stdio (stdin/stdout)
  for harness use. It ignores `--bind`/`--json`; stdout stays pure
  JSON-RPC (logs/errors go to stderr). The DB defaults to
  `./.ilearned/ilearned.db` under the working directory when `--db` /
  `ILEARNED_DB` are unset (parent dirs are created on open). See
  [docs/mcp.md](docs/mcp.md).
- `delete` on a never-existing `(topic, id)` exits 1 (not-found);
  deleting an already-deleted record is idempotent and succeeds.
- Failures write a structured error to stderr (`{"error": "..."}` with
  `--json`, `error: ...` otherwise) and return a non-zero exit code:
  0 ok, 1 not-found, 2 invalid input (incl. bad FTS syntax, unconfirmed
  destructive ops, and clap usage errors), 3 embedding unavailable,
  4 internal (incl. storage errors).
