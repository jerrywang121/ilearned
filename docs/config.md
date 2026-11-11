# Configuration

ilearned reads configuration in the environment and working directory of each
CLI or server process. Use `ilearned config show` from the same working directory
and environment as the process you are configuring to inspect the effective
settings and database path.

## Configuration files and precedence

ilearned loads these TOML files, merging values per field:

| Layer | Path |
| --- | --- |
| Global | `$XDG_CONFIG_HOME/ilearned/config.toml`, or `~/.config/ilearned/config.toml` when `XDG_CONFIG_HOME` is unset or empty (if `HOME` is also unset, the config path is `./.config/ilearned/config.toml`) |
| Workspace-local | `./.ilearned/config.toml`, relative to the process working directory |
| Explicit overlay | The path passed with `--config-file PATH`; it must exist |

Missing default files are ignored. The explicit `--config-file` is loaded after
the global and local files and overrides fields it sets. Environment values
override file values. For non-database settings, the effective order is:

```text
serve --bind (bind only) > environment > --config-file overlay > local file > global file > defaults
```

`--config-file` selects an additional file for that CLI invocation; it is not a
setting override by itself. Relative file and database paths resolve from the
process working directory, not from the directory containing the TOML file.
Malformed TOML, unknown keys, and invalid environment values fail rather than
being silently ignored.

## Database path selection

Database selection has its own precedence:

1. A non-empty `ILEARNED_DB`.
2. The merged `db` value from the configuration files.
3. If neither is configured, an existing `./.ilearned/ilearned.db` in the
   process working directory.
4. Otherwise, an existing `$XDG_DATA_HOME/ilearned/ilearned.db` or
   `~/.local/share/ilearned/ilearned.db`.

Fallback paths are used only when their database file already exists. If no
configured path or existing fallback is found, startup fails with
`db path is not configured`; ilearned does not silently create a fallback
database. An explicitly configured path may be new: the first database-using
command creates the database and any missing parent directories.

## Initialize and inspect configuration

Run local initialization from the workspace that should own the memory:

```bash
ilearned config init
```

This creates `./.ilearned/config.toml` with
`db = "./.ilearned/ilearned.db"`. It also creates
`./.ilearned/.gitignore` containing `*` if that file does not exist, so the
workspace-local memory files are ignored by Git. An existing `.gitignore` is
left untouched.

For user-global settings and storage, run:

```bash
ilearned config init -g
```

This writes the global TOML path above and configures the database under
`$XDG_DATA_HOME/ilearned/ilearned.db` or `~/.local/share/ilearned/ilearned.db`.
If neither `XDG_DATA_HOME` nor `HOME` is available, global initialization
cannot select a database path and fails. Global initialization does not create
a workspace-local `.ilearned/` directory.

Both forms create missing config directories and use owner-only permissions for
the TOML file where supported. They refuse to overwrite an existing config
unless `--force` (or `-f`) is supplied. Initialization writes configuration;
on success, it prints only the generated config path. It does not open or create
the database file. The database is created on the first `mcp`, `serve`, or CLI
data command that opens it.

Inspect the effective result with:

```bash
ilearned config show
```

The JSON output includes the resolved database path, settings, and
`config_files` that participated in resolution; `db` is null if no database can
be selected. `config show` does not open the database. It never prints embedding
API-key values; it reports only whether one is configured.

## Settings and environment variables

All TOML keys are optional. Environment variables override the corresponding
file setting. `serve --bind` overrides `bind` for that invocation.

| TOML setting | Environment variable | Default / behavior |
| --- | --- | --- |
| `db` | `ILEARNED_DB` | No configured default; see [database path selection](#database-path-selection) |
| `bind` | `ILEARNED_BIND` | `127.0.0.1:8787` |
| `active_days` | `ILEARNED_ACTIVE_DAYS` | `60` days |
| `forget_days` | `ILEARNED_FORGET_DAYS` | `120` days |
| `retention_days` | `ILEARNED_RETENTION_DAYS` | `60` days |
| `auto_delete_threshold` | — | `0.3`, TOML only; valid range `0.0..=1.0` |
| `[embedding].endpoint` | `ILEARNED_EMBED_ENDPOINT` | No default; required to enable embeddings |
| `[embedding].model` | `ILEARNED_EMBED_MODEL` | No default; required to enable embeddings |
| `[embedding].api_key` | `ILEARNED_EMBED_API_KEY` | No default; required to enable embeddings |
| `[embedding].dims` | `ILEARNED_EMBED_DIMS` | `1536` |
| `[embedding].timeout_secs` | `ILEARNED_EMBED_TIMEOUT_SECS` | `30` seconds |

The embedding provider is enabled only when endpoint, model, and API key are all
configured. `config show` does not reveal the API key value.

After a `demote`, ilearned auto-deletes a record if its feedback score is
strictly below `auto_delete_threshold`. A value of `0.0` disables automatic
deletion; `1.0` deletes on the first demote.

`ILEARNED_DB_KEY` is separate from the TOML settings and is the only source for
database encryption keys. When unset, SQLite remains plaintext for compatibility;
when set, the key is applied before database schema access. It is never written
to config, accepted as a command-line argument, logged, or included in errors or
output. Supplying a key does not convert an existing plaintext database;
conversion is explicit via `ilearned db encrypt` (see the [CLI guide](cli.md)).
Empty or non-UTF-8 values are invalid, and `config show` intentionally bypasses
database-key resolution.

Example TOML:

```toml
db = "./.ilearned/ilearned.db"
bind = "127.0.0.1:8787"
active_days = 60
forget_days = 120
retention_days = 60
auto_delete_threshold = 0.3

[embedding]
# endpoint = "http://localhost:11434/v1"
# model = "your-embedding-model"
# api_key = "your-api-key"
dims = 1536
timeout_secs = 30
```
