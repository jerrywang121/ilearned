# REST API

| Method | Route | Operation |
| --- | --- | --- |
| `GET` | `/healthz` | Readiness; verifies SQLite reachable + migrations applied |
| `GET` | `/api/v1/experiences` | Search/browse (`topic,text,semantic,limit,offset,deep`) |
| `POST` | `/api/v1/experiences` | Add |
| `PATCH` | `/api/v1/experiences/{topic}/{id}` | Modify selected fields |
| `DELETE` | `/api/v1/experiences/{topic}/{id}` | Delete one |
| `POST` | `/api/v1/experiences/{topic}/{id}/promote` | Positive feedback |
| `POST` | `/api/v1/experiences/{topic}/{id}/downgrade` | Negative feedback |
| `DELETE` | `/api/v1/experiences` | Clear by topic or all (explicit confirmation parameter) |

- Bodies/query use public field names (`when,if,do,check`); timestamps
  RFC 3339. Default pagination `limit=20 offset=0`.
- Errors: 400 validation (incl. bad FTS syntax), 404 missing (incl.
  `deleted`), 503 embedding required-but-unavailable, 500 internal.
  Body `{"error": "..."}`.
- No authentication in this local-first release.
