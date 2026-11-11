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
- Configuration and database selection follow ilearned's normal resolution
  rules; see the [configuration guide](config.md). For stdio, the process
  working directory and environment determine which local config and relative
  database path it uses. The MCP protocols expose no configuration controls.
- Harness config (opencode example):
  ```json
  { "mcp": { "ilearned": { "type": "local",
    "command": ["ilearned", "mcp"],
    "enabled": true } } }
  ```
  Run with the project directory as CWD when using workspace-local storage.
  See the [configuration guide](config.md) for other database locations.
- Tools (9): `search`, `add`, `update`, `delete`, `promote`, `demote`,
  `clear`, `topics_list`, `topics_search`. Argument schemas mirror the domain commands exactly:
  `search{topic?,text?,semantic?,limit?,offset?,deep?}`,
  `add{topic,do,when?,if?,check?}` (`if`/`do` are serde-renamed `if_text`/
  `do_text` fields), `update{topic,id,when?,if?,do?,check?}`,
  `delete/promote/demote{topic,id}`, `clear{topic?,all?,confirm}`
  (destructive `clear` requires `confirm: true` plus exactly one of
  `topic`/`all=true`, else `invalid_params`),
  `topics_list{level?,limit?,offset?,deep?}`,
  `topics_search{query,level?,limit?,offset?,deep?}` (`query` is a
  substring or `#` multi-level wildcard pattern).
  Topics are hierarchical (`travel/hotel/checkout`, segments `[a-z0-9_-]`).
  Add requires a valid `topic` and non-blank `do`; omitted or blank optional
  fields are stored as empty strings. For example,
  `{"topic":"fact","do":"remember Bob likes Blues"}`. On update, omitted
  fields stay unchanged; empty or whitespace-only `when`, `if`, or `check`
  clears it. Blank `do` is ignored and no effective update is invalid. JSON
  `null` is treated as omitted. The same rules apply to both transports.
  Mutation results use these JSON shapes: `add` returns
  `{"added":{"topic":"...","id":"..."}}`; `update` returns
  `{"modified":{"topic":"...","id":"..."}}`; `delete` returns
  `{"deleted":{"topic":"...","id":"..."}}`; `promote` and `demote`
  return `{"modified":{"topic":"...","id":"...","good_count":N,"bad_count":M,"state":"..."}}`
  (`state` is `"deleted"` when a demote crossed the auto-delete threshold);
  and `clear` returns
  `{"cleared":{"num_of_topics":N,"num_of_items":M}}`, where
  `num_of_topics` counts unique topics.
- Error mapping (actual): `InvalidInput`/`InvalidFtsSyntax` →
  `invalid_params`; `NotFound` → `resource_not_found`;
  `EmbeddingUnavailable` → `internal_error` with a `503`-equivalent
  message prefix; `Storage`/`Internal` → `internal_error`.
- Server identity: `initialize` reports `serverInfo.name=ilearned` with
  usage instructions.
- The tools type is currently concrete over `SqliteRepo` (rmcp macros
  require a non-generic impl); tool logic still calls the same
  `MemoryService` methods as CLI/REST — no MCP-specific business rules.
