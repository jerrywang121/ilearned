# Web server

`ilearned serve` starts REST, web, and MCP on one listener
(default `127.0.0.1:8787`).

- Configuration: database path, bind address, active/forget/retention
  periods, embedding settings. Precedence: flags > `ILEARNED_*` env >
  defaults. Default DB `./ilearned.db`.
- A non-loopback bind is an explicit deployment choice and does not imply
  authentication exists.
- Web routes (server-rendered HTML, no JS): search/browse `/`, add form,
  edit form, experience detail, feedback actions, delete action,
  clear-by-topic/all flow. Destructive actions require explicit
  confirmation. Handlers call `MemoryService` directly, never REST
  internally; HTML escaping and form validation are mandatory.
- Co-hosted surfaces: REST under `/api/v1/...` (see `rest-api.md`),
  MCP under `/mcp` (see `mcp.md`).
