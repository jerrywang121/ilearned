# CLI

```text
ilearned [--config-file PATH] <command>

ilearned add --topic TOPIC --do TEXT [--when TEXT] [--if TEXT] [--check TEXT] [--json]
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
ilearned db encrypt [--yes] [--json]
ilearned embedding migrate [--prune] [--yes] [--json]
ilearned config show
ilearned config init [-g] [--force | -f]
ilearned serve [--bind ADDR]
ilearned mcp
```

- The only global flag, `--config-file PATH`, selects an explicit configuration
  overlay for one CLI invocation. See the [configuration guide](config.md) for
  config-file paths, precedence, database selection, settings, and
  `config init` / `config show`.
  `--json` is a per-command output option (for example, `search --json`), not a
  global option. It is available only where the CLI has a selectable
  human-vs-JSON result; `serve`/`mcp` have protocol output, and `export` is
  always JSONL. REST, web, and MCP protocols expose no configuration controls.
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
- `embedding migrate` rebuilds derived vectors with the currently configured
  provider. It processes every non-deleted experience, including inactive and
  forgotten records, and excludes deleted records. Vectors are identified by
  `(model, dims)`; returned dimensions must match the configured dimension.
  Existing vector identities remain in place while the target identity is
  staged. A provider, dimension, or storage failure stops the run without
  pruning, while successful staged rows remain for a safe retry. On success,
  JSON output is
  `{"embedding_migration":{"model":"...","dims":N,"total":N,"migrated":N,"pruned":N}}`.
  Human output reports the same counts (`dims=unknown` is used when a custom
  provider cannot declare a dimension). `--prune` removes non-target vector
  identities only after success and requires `--yes` or an interactive `y/N`
  confirmation; refusal exits 2 and does not run the migration.
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
- `add` requires a valid topic and non-blank `--do`; `--when`, `--if`, and
  `--check` are optional. Missing or whitespace-only optional fields are stored
  as empty strings. For example: `ilearned add --topic fact --do "remember Bob likes Blues"`.
- `update` leaves omitted fields unchanged. To clear an optional field, pass an
  empty or whitespace-only value, for example `--when ""` or `--check ""`.
  Blank `--do` is ignored; an update with no effective fields is rejected.
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
  lines still commit. Optional `when`, `if`, and `check` may be absent or blank
  on import and become empty strings; exported empty values round-trip. `topic`,
  `id`, and non-blank `do` remain required. Exit 0 when clean, exit 2 when
  `E > 0`.
- `db encrypt [--yes] [--json]` explicitly converts the resolved plaintext
  database using `ILEARNED_DB_KEY` (see [configuration](config.md) for key
  configuration). It requires the normal destructive-action
  confirmation (`--yes` or interactive `y/N`) and runs before normal keyed
  service construction. Missing key, refusal, an already-encrypted/corrupt
  source leaves the source unchanged; later errors preserve logical contents if
  rollback succeeds. If Windows filesystem errors also prevent rollback, the
  error identifies the possible plaintext backup path. JSON success is `{"encrypted":{"path":"..."}}`; human
  output reports only the encrypted path. The source must be a regular file,
  not a symbolic link, and have a single hard link. Conversion uses
  `sqlcipher_export`,
  verifies integrity and schema, removes stale plaintext `-wal`/`-shm` sidecars,
  then replaces the source with `fs::rename` on Unix and other non-Windows
  targets or with a staged replacement and rollback attempt on Windows. It does
  not retain a plaintext
  backup on success; JSONL exports are not encrypted by this feature.
- `serve` starts REST + web + MCP on one listener. `--bind` on the `serve`
  subcommand wins over every config-file and environment value. After the
  listener is ready, startup prints the Web UI URL (`/`), REST API URL
  (`/api/v1`), and MCP URL (`/mcp`) for the bound address.
- `mcp` runs the same 9 tools as an MCP server over stdio (stdin/stdout)
  for harness use. It has no bind setting; stdout stays pure JSON-RPC
  (logs/errors go to stderr). It follows the database resolution rules above:
  configured paths win, otherwise existing local/global fallback files are
  used, and startup fails if no database can be resolved. See
  [MCP guide](mcp.md).
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
