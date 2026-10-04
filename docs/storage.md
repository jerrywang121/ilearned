# Storage

- The bundled rusqlite build includes SQLCipher and vendored OpenSSL. The
  shared opener accepts the optional `ILEARNED_DB_KEY`: when unset, databases
  remain plaintext SQLite for compatibility; when set, it applies the key
  before any schema access and enables `PRAGMA cipher_memory_security = ON`.
  SQLCipher's default version-4 settings are used. The key is environment-only
  and is never persisted, logged, returned, or accepted as a CLI argument.
- A present-but-empty or non-UTF-8 `ILEARNED_DB_KEY` is invalid input. A wrong
  key, or a key used against an existing plaintext database, is a database-key
  startup error (CLI exit 4) and does not trigger automatic conversion. The
  error points users to `ilearned db encrypt` without exposing key material.
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
  `(topic,id,model,dims)` (`vec` is a little-endian f32 blob; `dims` is the
  vector length). The table is created lazily by `ensure_table` on first
  vector op — it is NOT part of the base migrations and has NO foreign key to
  `experiences`. If an existing database has the former model-only primary
  key, `ensure_table` rebuilds the table transactionally and preserves all
  rows. Exact model-and-dimension filtering prevents incompatible vectors
  from being loaded together. Embedding rows for expired records are removed
  explicitly by `purge_expired` alongside the experience and FTS rows (there
  is no cascade); migration pruning removes non-target vector identities only
  after a complete successful re-embedding run.
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

## Plaintext-to-encrypted migration

`ilearned db encrypt [--yes] [--json]` is an explicit, confirmation-gated
conversion of the resolved existing regular plaintext database. It requires a
non-empty `ILEARNED_DB_KEY`; it never creates a missing source and refuses
already-encrypted, corrupt, or unsupported files without rewriting them.

The migration checkpoints/truncates plaintext WAL state, creates a temporary
destination in the same directory, attaches it with the key, and runs
`sqlcipher_export('encrypted')`. This copies the complete schema and contents,
including FTS tables/triggers and lazily-created embedding rows. It then opens
the destination with the key, runs SQLCipher integrity and ilearned schema
checks, and only after verification atomically replaces the source. Temporary
output is cleaned on all failure paths; stale plaintext `-wal`/`-shm` sidecars
are removed after success and no plaintext backup is retained.

Confirmation refusal, missing/invalid key, or any migration failure leaves the
original database unchanged. The successful result reports only the path; the
key is never included. JSONL `export` remains a plaintext portable backup and
is not encrypted by this feature.
