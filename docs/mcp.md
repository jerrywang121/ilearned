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
  `delete/promote/downgrade{topic,id}`, `clear{topic?,all?}`.
  Results are the same JSON shapes as REST/CLI.
- Error mapping (actual): service failures surface as MCP `internal_error`
  carrying the typed `AppError` message (e.g. `experience not found:
  (topic, id)`, `embedding provider unavailable: ...`), so callers match
  on the message text. The one exception is `clear` argument validation
  (neither/both of `topic`/`all`), which returns `invalid_params`.
- The tools type is currently concrete over `SqliteRepo` (rmcp macros
  require a non-generic impl); tool logic still calls the same
  `MemoryService` methods as CLI/REST — no MCP-specific business rules.
