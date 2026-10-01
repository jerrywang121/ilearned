# CLI

```text
ilearned [--config-file PATH] <command>

ilearned add --topic TOPIC --when TEXT --if TEXT --do TEXT --check TEXT [--json]
ilearned search [--topic TOPIC] [--text MATCH] [--semantic QUERY]
                  [--limit N] [--offset N] [--deep] [--json]
ilearned topic list [--level N] [--limit N] [--offset N] [--deep] [--json]
ilearned topic search QUERY [--level N] [--limit N] [--offset N] [--deep] [--json]
ilearned update --topic TOPIC --id ID [--when TEXT] [--if TEXT]
                  [--do TEXT] [--check TEXT] [--json]
ilearned delete --topic TOPIC --id ID [--yes] [--json]
ilearned promote --topic TOPIC --id ID [--json]
ilearned demote --topic TOPIC --id ID [--json]
ilearned clear (--topic TOPIC | --all) [--yes] [--json]
ilearned export [--topic TOPIC] [--deep] [--file PATH]
ilearned import [--file PATH] [--merge] [--json]
ilearned config show
ilearned config init [-g]
ilearned serve [--bind ADDR]
ilearned mcp
```

- The only global flag is `--config-file PATH`. It overlays the global
  `~/.config/ilearned/config.toml` and local `./.ilearned/config.toml` files
  for that CLI invocation; the selected file must exist. Environment values
  override file values. `--json` is a per-command output option (for example,
  `search --json`), not a global option. It is available only where the CLI
  has a selectable human-vs-JSON result; `serve`/`mcp` have protocol output,
  and `export` is always JSONL. REST, web, and MCP surfaces expose no
  configuration controls.
- `config show` always prints JSON with the effective `db`, `bind`, lifecycle,
  and embedding settings plus a `config_files` array containing the existing
  global, local, and explicit `--config-file` paths used during resolution.
  Embedding `api_key` values are never printed; the embedding object exposes
  `api_key_configured` instead. It does not open the database, so it also works
  before a database exists.
  `config init` generates the default TOML directly from the executable at
  `./.ilearned/config.toml` with `db = "./.ilearned/ilearned.db"` set as the
  active database path; `config init -g` uses the global config path
  (`$XDG_CONFIG_HOME/ilearned/config.toml` or `~/.config/ilearned/config.toml`)
  and sets `db` to the global data location
  (`$XDG_DATA_HOME/ilearned/ilearned.db`, falling back to
  `~/.local/share/ilearned/ilearned.db`; the command fails when neither
  `XDG_DATA_HOME` nor `HOME` is set). Parent directories are created, new
  files use owner-only permissions where supported, but an existing target is
  never overwritten and the command fails while reporting its path. On success
  it prints only the generated path.
- Topic form: hierarchical, e.g. `travel/hotel/checkout`; each `/`-separated
  segment must match `[a-z0-9_-]` (lowercase letters, digits, hyphen,
  underscore) — no empty segments, no uppercase, dots, or spaces. `#` is
  pattern-only and never stored. Every `--topic` help text documents this
  form. Write paths (`add`, `import`, `clear --topic`) validate the
  canonical form; `search`/`export --topic` accept `#` multi-level wildcard
  patterns: `travel/#` matches `travel` and everything under it, `#` alone
  matches everything, `#/checkout` and `travel/#/checkout` match with `#`
  in the middle. A bare `travel` matches only the exact topic (descendants
  need `travel/#`). `clear --topic` stays exact-match and rejects `#`.
- Lifecycle tuning uses `active_days`, `forget_days`, and `retention_days` in
  TOML or `ILEARNED_ACTIVE_DAYS`, `ILEARNED_FORGET_DAYS`, and
  `ILEARNED_RETENTION_DAYS` in the environment (defaults 60 / 120 / 60).
  `auto_delete_threshold` (TOML only, no env var; default `0.3`) auto-deletes
  a record when its feedback score `good_count / (good_count + bad_count)`
  drops strictly below the threshold after a `demote`; `0.0` disables
  auto-delete and `1.0` deletes on the first demote. Values outside
  `0.0..=1.0` are rejected with exit 2.
  Precedence is `serve --bind` (for bind only) > env > explicit config-file
  overlay > local file > global file > defaults.
- Database path resolution uses a configured value first: `ILEARNED_DB`, then
  the merged `db` value from the global/local/explicit config files. If no
  database is configured, an existing `./.ilearned/ilearned.db` is used; if it
  does not exist, an existing `$XDG_DATA_HOME/ilearned/ilearned.db` (or
  `~/.local/share/ilearned/ilearned.db`) is used. If neither fallback exists,
  startup fails with `db path is not configured` (exit 2). Resolution does not
  create a fallback file; explicitly configured paths may be created when the
  database is opened.
- Config files (TOML, all keys optional): global
  `~/.config/ilearned/config.toml` (`$XDG_CONFIG_HOME` respected), overlaid
  per-field by local `./.ilearned/config.toml`, then by the explicit
  `--config-file PATH` layer. Missing default files are ignored; the explicit
  file must exist. Malformed TOML or unknown keys abort with exit 2; an
  unparseable `bind` or numeric environment setting aborts with exit 2.

  ```toml
  db = "./ilearned.db"
  bind = "127.0.0.1:8787"
  active_days = 60
  forget_days = 120
  retention_days = 60
  auto_delete_threshold = 0.3
  [embedding]
  endpoint = "http://localhost:11434/v1"
  model = "nomic-embed-text"
  api_key = "secret"
  dims = 768
  timeout_secs = 30
  ```
  
- Embedding provider (optional): use `[embedding] endpoint`, `model`,
  `api_key`, `dims`, and `timeout_secs` in TOML or the corresponding
  `ILEARNED_EMBED_*` environment variables (defaults 1536 dimensions / 30s).
  When endpoint+model+key are all present, the binary builds an
  OpenAI-compatible provider and `search --semantic` works; otherwise
  semantic queries exit 3.
- In JSON mode, `add` returns `{"added":{"topic":"...","id":"..."}}`,
  `update` returns `{"modified":{"topic":"...","id":"..."}}`, and
  `delete` returns `{"deleted":{"topic":"...","id":"..."}}` to confirm
  the impacted record. `promote`/`demote` return
  `{"modified":{"topic":"...","id":"...","good_count":N,"bad_count":M,"state":"..."}}`;
  `state` is `"deleted"` when a demote crossed the auto-delete threshold.
  Human output adds only a short action label (demote appends
  `[auto-deleted]` when the record was auto-deleted). `search` prints a JSON array in
  `--json` mode.
  `limit` defaults to 20 and is clamped to `MAX_LIMIT=100` service-side
  (larger values behave as 100, no error); `limit=0` and over-range
  `offset` return `[]`. `text` + `semantic` together fuse both rankings
  with RRF (`k=60`); a semantic query without a provider (or with a
  failing provider) exits 3 instead of silently degrading to text-only.
- `update` requires at least one non-blank field of
  `--when/--if/--do/--check` (blank-only values are ignored, so all-blank
  is rejected).
- Destructive `delete`/`clear` require explicit confirmation: `--yes` or an
  interactive `y/N` prompt on stderr. Refusal (including EOF on stdin
  without `--yes`) aborts with exit code 2 and changes nothing.
- `clear` requires exactly one of `--topic` / `--all` (enforced by clap;
  violations exit 2). JSON success is
  `{"cleared":{"num_of_topics":N,"num_of_items":M}}`; `num_of_topics` is
  the number of unique topics affected. Human output is
  `cleared N topic(s), M experience(s)`.
- `export` always dumps experiences as JSONL (one `Experience` object per line in
  the REST wire shape: `topic/id/when/if/do/check`, RFC 3339 `updated_at`,
  `good_count`/`bad_count`/`state`). Optional `--topic` filter; `--deep`
  includes inactive (`deleted`/`forgotten` are always excluded, same as
  search). `--file PATH` writes to the file (parent dirs created,
  overwritten); without `--file` the JSONL goes to stdout for piping.
- `import` loads JSONL (one `Experience` object per line) from `--file`
  PATH or stdin, tries every line, and reports
  `{"new": N, "updated": M, "errors": E}` (human:
  `imported N new, M updated, E error(s)`). `--merge` keeps file ids and
  overwrites on `(topic, id)` collision (updating even soft-deleted rows
  in place); without `--merge` every line gets a fresh id. Malformed or
  invalid lines are skipped with per-line errors on stderr; the good
  lines still commit. Exit 0 when clean, exit 2 when `E > 0`.
- `serve` starts REST + web + MCP on one listener. `--bind` on the `serve`
  subcommand wins over every config-file and environment value.
- `mcp` runs the same 9 tools as an MCP server over stdio (stdin/stdout)
  for harness use. It has no bind setting; stdout stays pure JSON-RPC
  (logs/errors go to stderr). It follows the database resolution rules above:
  configured paths win, otherwise existing local/global fallback files are
  used, and startup fails if no database can be resolved. See
  [docs/mcp.md](docs/mcp.md).
- `delete` on a never-existing `(topic, id)` exits 1 (not-found);
  deleting an already-deleted record is idempotent and succeeds.
- `topic list` prints existing topics (one per line; JSON: array of strings),
  honoring visibility (`deleted`/`forgotten` never contribute, `inactive`
  only with `--deep`). `--level N` truncates each topic to its first N
  segments then dedups (`--level 2` shows `travel/hotel`, not
  `travel/hotel/checkout`). `topic search QUERY` filters by substring
  (lowercased before matching, so case-insensitive), or by `#` pattern
  when QUERY contains `#` (matched against the
  full topic before truncation). Both paginate with `--limit` (default 20,
  clamped to 100) / `--offset`; `--level 0` is rejected (exit 2).
- Failures write a structured error to stderr (`{"error": "..."}` with
  `--json`, `error: ...` otherwise) and return a non-zero exit code:
  0 ok, 1 not-found, 2 invalid input (incl. bad FTS syntax, unconfirmed
  destructive ops, and clap usage errors), 3 embedding unavailable,
  4 internal (incl. storage errors).
