# REST API

| Method | Route | Operation | Success status |
| --- | --- | --- | --- |
| `GET` | `/healthz` | Readiness; runs a zero-limit search to verify SQLite reachable + migrations applied | 200 `{"ok":true}` |
| `GET` | `/api/v1/experiences` | Search/browse (`topic,text,semantic,limit,offset,deep`) | 200 array |
| `POST` | `/api/v1/experiences` | Add | 201 record |
| `PATCH` | `/api/v1/experiences/:topic/:id` | Modify selected fields (≥1 required) | 200 record |
| `DELETE` | `/api/v1/experiences/:topic/:id` | Delete one (idempotent; missing rows still succeed) | 204, no body |
| `POST` | `/api/v1/experiences/:topic/:id/promote` | Positive feedback (`good_count+1`) | 200 record |
| `POST` | `/api/v1/experiences/:topic/:id/downgrade` | Negative feedback (`bad_count+1`) | 200 record |
| `DELETE` | `/api/v1/experiences` | Clear by topic or all; requires `?confirm=true` plus exactly one of `?topic=X` / `?all=true` | 200 `{"cleared": N}` |

- Bodies/query use public field names (`when,if,do,check`); timestamps
  RFC 3339. Add example:
  `{"topic":"rust","when":"...","if":"...","do":"...","check":"..."}`
  (all five required, blanks rejected). Modify accepts any subset of
  `when/if/do/check`.
- Pagination: default `limit=20 offset=0`; `limit` is clamped to
  `MAX_LIMIT=100` (larger values behave as 100, no error); `limit=0` and
  over-range `offset` return `[]`.
- `deep=true` includes `inactive` records; `deleted`/`forgotten` are never
  returned. Deleted records otherwise read as 404.
- Errors: 400 validation (incl. bad FTS syntax, empty modify, clear
  without `confirm=true` or with both/neither of `topic`/`all`), 404
  missing (incl. `deleted`), 503 embedding required-but-unavailable, 500
  internal (incl. storage). Body `{"error": "..."}`.
- Axum 0.7 path syntax (`:topic/:id`); URL-encoded topics/ids are decoded
  by the extractor.
- No authentication in this local-first release.
