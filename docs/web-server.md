# Web server

`ilearned serve [--bind ADDR]` starts REST, web, and MCP on one listener
(default `127.0.0.1:8787`; subcommand `--bind` wins over every config-file
and `ILEARNED_BIND` value). Database comes from the selected/default TOML
files or `ILEARNED_DB`; when unset, an existing `./.ilearned/ilearned.db` is
preferred, followed by an existing XDG data database
(`$XDG_DATA_HOME/ilearned/ilearned.db`, or
`~/.local/share/ilearned/ilearned.db`). Startup errors if neither fallback
exists. The `--config-file` selector and all other configuration are CLI-only;
REST, web, and HTTP MCP do not expose configuration controls.

- Lifecycle periods default to active 60d / forget 120d / retention 60d and
  are tunable via `active_days` / `forget_days` / `retention_days` in TOML or
  `ILEARNED_ACTIVE_DAYS` / `ILEARNED_FORGET_DAYS` /
  `ILEARNED_RETENTION_DAYS`. When embedding TOML/env values (`endpoint`,
  `model`, and `api_key`, plus optional `dims` / `timeout_secs`) are all
  present, `serve` builds an OpenAI-compatible provider so semantic queries
  work; otherwise they return 503 / exit 3.
- A non-loopback bind is an explicit deployment choice and does not imply
  authentication exists.
- Web routes (server-rendered HTML via Askama, inline CSS, no JS),
  all calling `MemoryService` directly (never REST internally):

  | Method | Route | Notes |
  | --- | --- | --- |
  | `GET` | `/` | Search/browse; query `q,topic,deep,limit,offset` |
  | `GET` | `/experiences/new` | Add form |
  | `POST` | `/experiences` | Add submit; form fields `topic,when_text,if_text,do_text,check` (all required, blanks → 400) |
  | `GET` | `/experiences/:topic/:id` | Detail via `MemoryService::get` (sole entry; deleted/missing → 404) |
  | `GET` | `/experiences/:topic/:id/edit` | Edit form |
  | `POST` | `/experiences/:topic/:id` | Edit submit; non-blank subset of `when_text/if_text/do_text/check`; empty → 400 |
  | `POST` | `/experiences/:topic/:id/promote` | Feedback (`good_count+1`, restores to active), redirects to detail |
  | `POST` | `/experiences/:topic/:id/demote` | Feedback (`bad_count+1`, restores to active; auto-deletes below `auto_delete_threshold`), redirects to detail, or to the list when auto-deleted |
  | `POST` | `/experiences/:topic/:id/delete` | Requires `confirm=yes` field, else 400; redirects to `/` |
  | `GET/POST` | `/clear` | Requires `confirm=yes` plus a topic or `all=true`, else 400 |
  | `GET` | `/topics` | Topic list/search page; query `q,level,deep,limit,offset`; linked from `/` |

- Since topics may contain `/`, links and redirects percent-encode the topic as
  one path segment (for example, `jump/down/up` becomes
  `jump%2Fdown%2Fup`). Clients constructing detail or action URLs should encode
  topic slashes the same way.
- Askama auto-escaping is ON for all templates; error pages render the
  `AppError` message with the mapped status (400/404/503/500).
- Co-hosted surfaces: REST under `/api/v1/...` (see `rest-api.md`),
  MCP under `/mcp` (see `mcp.md`).
