# CLI

```text
ilearned [--db PATH] [--bind ADDR] [--json] <command>

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

- Global flags: `--db PATH` (`ILEARNED_DB`, default `./ilearned.db`),
  `--bind ADDR` (`ILEARNED_BIND`, default `127.0.0.1:8787`, only used by
  `serve`), `--json` for stable JSON output. Human output is for
  interactive use; JSON output is stable for agent and script use.
- `add` prints the created `Experience` (JSON: the record; human: one-line
  summary plus fields). `search` prints a JSON array in `--json` mode.
- `modify` requires at least one of `--when/--if/--do/--check`.
- Destructive `delete`/`clear` require explicit confirmation: `--yes` or an
  interactive `y/N` prompt on stderr. Refusal (including EOF on stdin
  without `--yes`) aborts with exit code 2 and changes nothing.
- `clear` requires exactly one of `--topic` / `--all` (enforced by clap;
  violations exit 2).
- `serve` starts REST + web + MCP on one listener. `--bind` on the `serve`
  subcommand wins over the global `--bind`.
- `search --semantic` without a configured provider exits 3; semantic
  support is not yet wired into the binary (no provider is constructed in
  `main`), so `--text`/browse are the working paths.
- Failures write a structured error to stderr (`{"error": "..."}` with
  `--json`, `error: ...` otherwise) and return a non-zero exit code:
  0 ok, 1 not-found, 2 invalid input (incl. bad FTS syntax, unconfirmed
  destructive ops, and clap usage errors), 3 embedding unavailable,
  4 internal (incl. storage errors).
