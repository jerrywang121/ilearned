# REST API

| Method | Route | Operation | Success status |
| --- | --- | --- | --- |
| `GET` | `/healthz` | Readiness; runs a zero-limit search to verify SQLite reachable + migrations applied | 200 `{"ok":true}` |
| `GET` | `/api/v1/experiences` | Search/browse (`topic,text,semantic,limit,offset,deep`) | 200 array |
| `POST` | `/api/v1/experiences` | Add | 201 record |
| `PATCH` | `/api/v1/experiences/:topic/:id` | Update selected fields (≥1 required) | 200 record |
| `DELETE` | `/api/v1/experiences/:topic/:id` | Delete one: never-existing id → 404; already-deleted → idempotent 204 | 204, no body |
| `POST` | `/api/v1/experiences/:topic/:id/promote` | Positive feedback (`good_count+1`; restores `inactive`/`forgotten` to active) | 200 record |
| `POST` | `/api/v1/experiences/:topic/:id/demote` | Negative feedback (`bad_count+1`; restores `inactive`/`forgotten` to active) | 200 record |
| `DELETE` | `/api/v1/experiences` | Clear by topic or all; requires `?confirm=true` plus exactly one of `?topic=X` / `?all=true` | 200 `{"cleared":{"num_of_topics":N,"num_of_items":M}}` |
| `GET` | `/api/v1/topics` | List/search distinct topics (`level,q,limit,offset,deep`); no `q` = list, with `q` = search | 200 array of strings |

- Clear responses report `num_of_items` newly transitioned to `deleted` and
  `num_of_topics` as the number of unique topics represented by those items.
- Bodies/query use public field names (`when,if,do,check`); timestamps
  RFC 3339. Add example:
  `{"topic":"rust","when":"...","if":"...","do":"...","check":"..."}`
  (all five required, blanks rejected). Update accepts any non-blank
  subset of `when/if/do/check` (blank-only values are ignored, so an
  all-blank update is a 400).
- Pagination: default `limit=20 offset=0`; `limit` is clamped to
  `MAX_LIMIT=100` (larger values behave as 100, no error); `limit=0` and
  over-range `offset` return `[]`.
- `text` + `semantic` together fuse both rankings with RRF (`k=60`,
  tie-break `updated_at DESC, topic ASC, id ASC`); a semantic query that
  cannot obtain an embedding fails typed (503), never silently
  text-only.
- `deep=true` includes `inactive` records; `deleted`/`forgotten` are never
  returned. Deleted records otherwise read as 404 (`GET` detail has no
  route — use search or `MemoryService::get`).
- Topics are hierarchical (`travel/hotel/checkout`, segments `[a-z0-9_-]`);
  experience `topic` filters accept `#` multi-level wildcard patterns
  (`travel/#`, `#/checkout`, `travel/#/checkout`); bare `travel` matches
  only the exact topic.
- `GET /api/v1/topics?level=&q=&limit=&offset=&deep=`: distinct topics
  sorted ascending, same visibility as search. `q` is a substring
  (lowercased before matching, so case-insensitive), or a `#` pattern
  when it contains `#` (matched against the
  full topic before truncation; `#` must be URL-encoded as `%23`). `level`
  truncates to the first N segments then dedups (`level=0` → 400).
  Pagination mirrors search (default `limit=20 offset=0`, clamp 100).
- Errors: 400 validation (incl. bad FTS syntax, empty update, clear
  without `confirm=true` or with both/neither of `topic`/`all`), 404
  missing (incl. `deleted`, plus `DELETE`/`PATCH` on never-existing ids),
  503 embedding required-but-unavailable, 500
  internal (incl. storage). Body `{"error": "..."}`. Malformed JSON
  (e.g. a missing `topic` key on add) is rejected by the extractor with
  422 before validation runs.
- Axum 0.7 path syntax (`:topic/:id`); URL-encoded topics/ids are decoded
  by the extractor.
- No authentication in this local-first release.
