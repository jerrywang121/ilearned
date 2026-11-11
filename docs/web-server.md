# Web server

`ilearned serve [--bind ADDR]` starts REST, web, and MCP on one listener
(default `127.0.0.1:8787`; subcommand `--bind` wins over configuration and
`ILEARNED_BIND`). See the [configuration guide](config.md) for config sources,
database selection, and settings. The `--config-file` selector and all other
configuration are CLI-only; REST, web, and HTTP MCP do not expose configuration
controls.

- Lifecycle and optional embedding settings are described in the
  [configuration guide](config.md). Without a configured embedding provider,
  semantic queries return 503 / exit 3.
- A non-loopback bind is an explicit deployment choice and does not imply
  authentication exists.
- Web routes (server-rendered HTML via Askama, inline CSS, no JS),
  all calling `MemoryService` directly (never REST internally):
  Page headers display `iLearned`, the Cargo package version, and a link to the
  GitHub repository.

  | Method | Route | Notes |
  | --- | --- | --- |
  | `GET` | `/` | Search/browse; query `q,topic,deep,limit,offset` |
  | `GET` | `/export` | JSONL download; `scope=all` or `scope=search` with `q,topic,deep` |
  | `GET` | `/experiences/new` | Add form |
  | `POST` | `/experiences` | Add submit; `topic` and non-blank `do_text` required; `when_text`, `if_text`, `check` optional |
  | `GET` | `/experiences/:topic/:id` | Detail via `MemoryService::get` (sole entry; deleted/missing → 404) |
  | `GET` | `/experiences/:topic/:id/edit` | Edit form |
  | `POST` | `/experiences/:topic/:id` | Edit submit; empty `when_text`, `if_text`, or `check` clears it; blank `do_text` ignored; no effective fields → 400 |
  | `POST` | `/experiences/:topic/:id/promote` | Feedback (`good_count+1`, restores to active), redirects to `/` preserving the request query |
  | `POST` | `/experiences/:topic/:id/demote` | Feedback (`bad_count+1`, restores to active; auto-deletes below `auto_delete_threshold`), redirects to `/` preserving the request query |
  | `POST` | `/experiences/:topic/:id/delete` | Requires `confirm=yes` field, else 400; redirects to `/` |
  | `GET/POST` | `/clear` | Requires `confirm=yes` plus a topic or `all=true`, else 400 |
  | `GET` | `/topics` | Topic list/search page; query `q,level,deep,limit,offset`; linked from `/` |

- The `/` search/browse page displays experiences in a topic-ascending table
  with `Topic`, `When`, `If`, `Do`, `Check`, and `Ops` columns. Each row offers
  `When`, `If`, `Do`, and `Check` text in a smaller font, capped at 110
  characters plus `…` when longer; full values are preserved. The `Ops` column
  is 5% of the table width and uses three lines: red `✘` delete
  then `✎` edit; `★` promote with the good count; and `☆` demote with the bad
  count. Feedback actions return to `/` with the current `q`, `topic`, `deep`,
  `limit`, and `offset`, preserving the current page. Search/topic filters are
  widened, and the table scrolls horizontally on narrow screens. The edit form
  uses full-width multiline textareas for `when`, `if`, `do`, and `check`.
- The list defaults to 20 results per page and provides Previous/Next controls
  with a page indicator. Navigation preserves `q`, `topic`, `deep`, and `limit`
  while advancing or rewinding `offset`; submitting new filters preserves
  `limit` and returns to the first page.
- The browse page offers **export all** and **export search results**. Export all
  ignores the current filters and includes active and inactive experiences.
  Export search results uses the applied text, topic (including wildcards), and
  deep filters and includes matching experiences from every page. Both exclude
  deleted and forgotten experiences and download CLI-compatible JSONL that can
  be restored with `ilearned import --file PATH`. Downloads are plaintext even
  when the database is encrypted; keep them secure.
- The add form matches the edit form's wide layout and full-width multiline
  textareas for `when`, `if`, `do`, and `check`; `topic` remains a single-line
  input. Only topic and `do` are required. Clearing an optional textarea on the
  edit form removes that field's value; omitted fields leave it unchanged.

- Since topics may contain `/`, links and redirects percent-encode the topic as
  one path segment (for example, `jump/down/up` becomes
  `jump%2Fdown%2Fup`). Clients constructing detail or action URLs should encode
  topic slashes the same way.
- Askama auto-escaping is ON for all templates; error pages render the
  `AppError` message with the mapped status (400/404/503/500).
- Co-hosted surfaces: REST under `/api/v1/...` (see `rest-api.md`),
  MCP under `/mcp` (see `mcp.md`).
