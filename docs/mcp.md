# MCP

- Transport: streamable HTTP at `/mcp` (same listener as REST/web), via
  `rmcp` `StreamableHttpService` with a local session manager. Full
  handshake required: `initialize` → `notifications/initialized`
  (with `mcp-session-id` + `Mcp-Protocol-Version` headers) → `tools/*`.
- Tools (7): `search`, `add`, `modify`, `delete`, `promote`, `downgrade`,
  `clear`. Argument schemas mirror the domain commands exactly:
  `search{topic?,text?,semantic?,limit?,offset?,deep?}`,
  `add{topic,when,if,do,check}` (`if`/`do` are serde-renamed `if_text`/
  `do_text` fields), `modify{topic,id,when?,if?,do?,check?}`,
  `delete/promote/downgrade{topic,id}`, `clear{topic?,all?,confirm}`
  (destructive `clear` requires `confirm: true`, else `invalid_params`).
  Results are the same JSON shapes as REST/CLI.
- Error mapping (actual): `InvalidInput`/`InvalidFtsSyntax` →
  `invalid_params`; `NotFound` → `resource_not_found`;
  `EmbeddingUnavailable` → `internal_error` with a `503`-equivalent
  message prefix; `Storage`/`Internal` → `internal_error`.
- The tools type is currently concrete over `SqliteRepo` (rmcp macros
  require a non-generic impl); tool logic still calls the same
  `MemoryService` methods as CLI/REST — no MCP-specific business rules.
