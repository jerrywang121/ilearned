# MCP

- Transports (same 9 tools on both):
  - Streamable HTTP at `/mcp` (same listener as REST/web), via `rmcp`
    `StreamableHttpService` with a local session manager. Full handshake
    required: `initialize` → `notifications/initialized`
    (with `mcp-session-id` + `Mcp-Protocol-Version` headers) → `tools/*`.
  - Stdio via `ilearned mcp`: MCP over stdin/stdout for harness use
    (e.g. opencode). Handshake is plain JSON-RPC lines: `initialize` →
    `notifications/initialized` → `tools/*`. Stdout stays pure JSON-RPC
    (all logs/errors go to stderr). No `--bind`, no session headers.
- Database: configured `db` values from `ILEARNED_DB` or the merged
  selected/default config files take precedence. When no database is
  configured, `ilearned mcp` uses an existing `./.ilearned/ilearned.db` under
  the harness's working directory, then an existing
  `$XDG_DATA_HOME/ilearned/ilearned.db` (or
  `~/.local/share/ilearned/ilearned.db`). If neither exists, startup fails
  with `db path is not configured`; no fallback file is created. Configuration
  selection is CLI-only; the HTTP and stdio MCP protocols expose no
  configuration controls.
  Add `.ilearned/` to `.gitignore` unless the memory should be shared.
- Harness config (opencode example):
  ```json
  { "mcp": { "ilearned": { "type": "local",
    "command": ["ilearned", "mcp"],
    "enabled": true } } }
  ```
  Run with the project dir as cwd when an existing local store should be used,
  or pass
  `--config-file <path>` (with `db = "..."`) / `ILEARNED_DB` for an explicit
  location.
- Tools (9): `search`, `add`, `modify`, `delete`, `promote`, `downgrade`,
  `clear`, `topics_list`, `topics_search`. Argument schemas mirror the domain commands exactly:
  `search{topic?,text?,semantic?,limit?,offset?,deep?}`,
  `add{topic,when,if,do,check}` (`if`/`do` are serde-renamed `if_text`/
  `do_text` fields), `modify{topic,id,when?,if?,do?,check?}`,
  `delete/promote/downgrade{topic,id}`, `clear{topic?,all?,confirm}`
  (destructive `clear` requires `confirm: true` plus exactly one of
  `topic`/`all=true`, else `invalid_params`),
  `topics_list{level?,limit?,offset?,deep?}`,
  `topics_search{query,level?,limit?,offset?,deep?}` (`query` is a
  substring or `#` multi-level wildcard pattern).
  Topics are hierarchical (`travel/hotel/checkout`, segments `[a-z0-9_-]`).
  Results are the same JSON shapes as REST/CLI.
- Error mapping (actual): `InvalidInput`/`InvalidFtsSyntax` →
  `invalid_params`; `NotFound` → `resource_not_found`;
  `EmbeddingUnavailable` → `internal_error` with a `503`-equivalent
  message prefix; `Storage`/`Internal` → `internal_error`.
- Server identity: `initialize` reports `serverInfo.name=ilearned` with
  usage instructions.
- The tools type is currently concrete over `SqliteRepo` (rmcp macros
  require a non-generic impl); tool logic still calls the same
  `MemoryService` methods as CLI/REST — no MCP-specific business rules.
