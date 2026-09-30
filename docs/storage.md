# Storage

- SQLite at a configured path, or (when no path is configured) an existing
  `./.ilearned/ilearned.db` followed by an existing XDG data database
  (`$XDG_DATA_HOME/ilearned/ilearned.db`, or
  `~/.local/share/ilearned/ilearned.db`). Startup errors if neither fallback
  exists. Explicitly configured paths may be created by `open_db`, which also
  enables WAL, sets `busy_timeout=5000`, and records
  `schema_migrations(version=1)`; the compound primary key is `(topic, id)`.
- `experiences` columns: `topic, id, when_text, if_text, do_text,
  check_text, updated_at (INTEGER epoch seconds), good_count, bad_count,
  state ('active'|'inactive'|'deleted'|'forgotten'), retention_started_at
  (INTEGER, nullable, internal — never in the public shape)`.
- FTS5 table `experiences_fts` over
  `topic, when_text, if_text, do_text, check_text` with after-insert/
  after-delete/after-update sync triggers plus a `rebuild_fts` recovery
  path; BM25 rank; topic filter applied outside the `MATCH` expression;
  lifecycle eligibility applied after the match. Invalid `MATCH` syntax
  maps to `AppError::InvalidFtsSyntax` (HTTP 400 / CLI exit 2).
- `embeddings(topic, id, model, dims, vec BLOB LE-f32)` keyed
  `(topic,id,model)` (`vec` is a little-endian f32 blob; `dims` is the
  vector length). The table is created lazily by `ensure_table` on
  first vector op — it is NOT part of the base migrations and has NO
  foreign key to `experiences`; embedding rows for expired records are
  removed explicitly by `purge_expired` alongside the experience and FTS
  rows (there is no cascade).
- `update()` clears `retention_started_at`; the service restores
  `inactive`/`forgotten` records to `active` with a fresh `updated_at`
  before calling it (on `update`/`promote`/`demote`).
  `soft_delete()`/`clear()` set `state='deleted'` + fresh `updated_at` +
  retention start. `clear` skips already-deleted rows and returns the number
  of newly touched items plus the number of unique topics containing them.
  `demote` auto-delete reuses `soft_delete()` when the feedback score
  `good_count / (good_count + bad_count)` drops strictly below
  `auto_delete_threshold` (default `0.3`, TOML-only), so the retention
  clock starts and purge applies as with explicit deletes.
- Lifecycle (`reconcile_before_op`, single `BEGIN IMMEDIATE` transaction):
  reconcile before every op (strict `>`: `>60d`→inactive within the
  forget window, `>120d`→forgotten + retention start via `COALESCE` —
  a very old `active` row goes straight to `forgotten`); then
  `purge_expired` physically removes `deleted`/`forgotten` rows whose
  retention start is strictly `>60d` old.
