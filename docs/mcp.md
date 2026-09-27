# MCP

- Transports (same 7 tools on both):
  - Streamable HTTP at `/mcp` (same listener as REST/web), via `rmcp`
    `StreamableHttpService` with a local session manager. Full handshake
    required: `initialize` → `notifications/initialized`
    (with `mcp-session-id` + `Mcp-Protocol-Version` headers) → `tools/*`.
  - Stdio via `ilearned mcp`: MCP over stdin/stdout for harness use
    (e.g. opencode). Handshake is plain JSON-RPC lines: `initialize` →
    `notifications/initialized` → `tools/*`. Stdout stays pure JSON-RPC
    (all logs/errors go to stderr). No `--bind`, no session headers.
- Database: `ilearned mcp` defaults to `./.ilearned/ilearned.db` under the
  harness's working directory when `--db`/`ILEARNED_DB` are unset
  (parent dirs are created on open), so each project gets a local store.
  Add `.ilearned/` to `.gitignore` unless the memory should be shared.
- Harness config (opencode example):
  ```json
  { "mcp": { "ilearned": { "type": "local",
    "command": ["ilearned", "mcp"],
    "enabled": true } } }
  ```
  Run with the project dir as cwd for the default local store, or pass
  `--db <path>` / `ILEARNED_DB` for an explicit location.
- Tools (7): `search`, `add`, `modify`, `delete`, `promote`, `downgrade`,
  `clear`. Argument schemas mirror the domain commands exactly:
  `search{topic?,text?,semantic?,limit?,offset?,deep?}`,
  `add{topic,when,if,do,check}` (`if`/`do` are serde-renamed `if_text`/
  `do_text` fields), `modify{topic,id,when?,if?,do?,check?}`,
  `delete/promote/downgrade{topic,id}`, `clear{topic?,all?,confirm}`
  (destructive `clear` requires `confirm: true` plus exactly one of
  `topic`/`all=true`, else `invalid_params`).
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
