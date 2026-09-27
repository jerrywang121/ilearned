# Web server

`ilearned serve [--bind ADDR]` starts REST, web, and MCP on one listener
(default `127.0.0.1:8787`; subcommand `--bind` wins over global `--bind` /
`ILEARNED_BIND`). Database comes from `--db` / `ILEARNED_DB`
(default `./ilearned.db`).

- Lifecycle uses fixed defaults (active 60d / forget 120d / retention 60d);
  there are currently no flags/env vars to tune them, and `serve` does not
  construct an embedding provider (semantic queries return 503 / exit 3).
- A non-loopback bind is an explicit deployment choice and does not imply
  authentication exists.
- Web routes (server-rendered HTML via Askama, inline CSS, no JS),
  all calling `MemoryService` directly (never REST internally):

  | Method | Route | Notes |
  | --- | --- | --- |
  | `GET` | `/` | Search/browse; query `q,topic,deep,limit,offset` |
  | `GET` | `/experiences/new` | Add form |
  | `POST` | `/experiences` | Add submit; form fields `topic,when_text,if_text,do_text,check` (all required, blanks → 400) |
  | `GET` | `/experiences/:topic/:id` | Detail (deleted → 404) |
  | `GET` | `/experiences/:topic/:id/edit` | Edit form |
  | `POST` | `/experiences/:topic/:id` | Edit submit; non-blank subset of `when_text/if_text/do_text/check`; empty → 400 |
  | `POST` | `/experiences/:topic/:id/promote` | Feedback, redirects to detail |
  | `POST` | `/experiences/:topic/:id/downgrade` | Feedback, redirects to detail |
  | `POST` | `/experiences/:topic/:id/delete` | Requires `confirm=yes` field, else 400; redirects to `/` |
  | `GET/POST` | `/clear` | Requires `confirm=yes` plus a topic or `all=true`, else 400 |

- Askama auto-escaping is ON for all templates; error pages render the
  `AppError` message with the mapped status (400/404/503/500).
- Co-hosted surfaces: REST under `/api/v1/...` (see `rest-api.md`),
  MCP under `/mcp` (see `mcp.md`).
