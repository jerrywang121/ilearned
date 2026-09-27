# Storage

- SQLite at configurable path (default `./ilearned.db`), WAL mode,
  compound primary key `(topic, id)`.
- `experiences` columns: `topic, id, when_text, if_text, do_text,
  check_text, updated_at (INTEGER epoch), good_count, bad_count,
  state ('active'|'inactive'|'deleted'|'forgotten'), retention_started_at
  (INTEGER, nullable, internal — never in the public shape)`.
- FTS5 table over `topic, when_text, if_text, do_text, check_text` with
  sync triggers + rebuild path; BM25 rank; topic filter outside FTS;
  lifecycle eligibility applied after match.
- `embeddings(topic, id, model, dims, vec)` keyed `(topic,id,model)`,
  cascade-deleted with the experience.
- Lifecycle: reconcile before every op (strict `>`: `>60d`→inactive,
  `>120d`→forgotten + retention start); purge `deleted`/`forgotten`
  with retention start `>60d` (incl. FTS + embedding rows).
