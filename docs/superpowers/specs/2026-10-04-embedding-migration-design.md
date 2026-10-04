# Embedding Model and Dimension Migration

## Status

Approved design for implementation on `feat/embedding-migration`.

## Problem

Experiences are canonical records, while semantic vectors are a derived
cache. Today an embedding row is keyed by `(topic, id, model)`. Changing the
configured embedding model leaves the old rows in SQLite, but semantic search
only loads rows for the newly configured model. Existing experiences therefore
lose semantic coverage until each one is manually updated. The stored
dimension is also not part of the lookup identity, and cosine similarity
currently truncates mismatched vectors to the shorter length.

The migration path must rebuild derived vectors without changing or risking
canonical experiences, handle a dimension change even when the model name is
unchanged, and make obsolete vector cleanup an explicit destructive action.

## Goals

- Provide an explicit local CLI command for rebuilding vectors with the
  currently configured embedding provider.
- Identify vector sets by both provider model and vector dimension.
- Migrate every non-deleted experience, including inactive and forgotten
  records, without modifying its canonical fields or lifecycle state.
- Preserve old vectors and successfully staged new vectors when a run fails.
- Make retries safe by allowing the command to be run again.
- Prune obsolete vector identities only after a complete successful run and
  explicit confirmation.
- Reject dimension mismatches instead of silently comparing truncated vectors.
- Upgrade existing SQLite databases without losing vectors.

## Non-goals

- Migrating, rewriting, or deleting canonical experiences.
- Adding an HTTP or MCP migration endpoint.
- Running migration automatically during search, add, update, or other normal
  operations.
- Maintaining multiple searchable embedding models at the same time. The
  configured provider's `(model, dimension)` remains the active search target.
- Providing a distributed job queue or a background worker.

## User-facing command

The CLI gains a nested command:

```text
ilearned embedding migrate [--prune] [--yes] [--json]
```

The command uses the provider resolved from the normal configuration and
environment precedence. A provider is required; when none is configured the
existing typed `EmbeddingUnavailable` error is returned with exit code 3.

Without `--prune`, the command only writes the target vector set. Existing
vectors for other identities remain available in SQLite but are ignored by
semantic search. With `--prune`, the command uses the existing destructive
confirmation behavior. `--yes` bypasses the prompt; without it, an
interactive `y/yes` response is required and EOF/refusal aborts the command.
Pruning is performed only after every migration embedding and vector write has
succeeded. A failed run never prunes, even when `--prune` was supplied.

Successful human output is a concise summary containing the target model,
target dimension, total candidate count, migrated count, and pruned row count.
JSON output has the corresponding stable object shape:

```json
{
  "embedding_migration": {
    "model": "example-model",
    "dims": 768,
    "total": 42,
    "migrated": 42,
    "pruned": 84
  }
}
```

The exact counts are computed by the service and are not inferred by the CLI.
No migration command is exposed through REST or MCP because this is an
operator-controlled database maintenance action.

## Architecture

### Embedding identity

Introduce an embedding identity consisting of:

```text
model: String
dims: usize
```

`EmbeddingProvider` gains an optional expected-dimension method with a
backward-compatible default for providers that cannot declare one. The
OpenAI-compatible provider returns the configured `EmbeddingConfig::dims`;
the deterministic fake provider returns 64. Every returned vector is checked
against the provider's declared dimension when one exists. If no expected
dimension is declared, the first successful migration vector establishes the
dimension for that run and all later vectors must match it.

The provider request format remains compatible with the existing
OpenAI-compatible endpoint. The configured dimension is a validation contract
for the returned vector; an endpoint that returns a different size produces a
typed embedding error rather than creating an ambiguous vector set.

The same validation is applied to normal best-effort write embeddings and to
semantic query embeddings. Canonical writes still succeed when a write
embedding fails; the failure is logged and no invalid vector is stored.

### Vector storage

The `embeddings` primary key changes from:

```text
(topic, id, model)
```

to:

```text
(topic, id, model, dims)
```

The `dims` column already exists and continues to store the vector length.
`VectorStore` changes are:

- `upsert_vector` upserts on all four identity columns.
- `load_vectors` accepts both model and dimension and returns only exact
  matches.
- `delete_vectors(topic, id)` continues to delete every identity for one
  experience so lifecycle purge remains complete.
- A pruning operation deletes rows where `(model, dims)` is not the supplied
  target and returns the number of deleted rows.

When the vector table does not exist, the new DDL creates it lazily as today.
When an existing table uses the old primary key, `ensure_table` performs a
SQLite table rebuild inside a transaction: create the new table, copy all
rows, drop the old table, and rename the new table. The existing rows and
their stored dimensions are preserved. No foreign key to `experiences` is
introduced.

`cosine` no longer contributes a score for mismatched dimensions. Semantic
search already loads only the query's exact identity; the ranking helper must
also avoid its previous shorter-vector truncation behavior for direct or
future callers.

### Application service

Add a migration method to `MemoryService`, keeping all lifecycle, provider,
storage, and ranking behavior behind the application boundary. The method:

1. Runs the normal lifecycle reconciliation once before selecting work.
2. Reads all experiences where `state != 'deleted'` through a new repository
   operation. This deliberately includes inactive and forgotten experiences;
   deleted records are excluded because they are not searchable and may be
   purged by retention.
3. Captures the provider model and expected dimension.
4. Embeds each experience's existing `when/if/do/check` document text.
5. Validates the vector dimension and upserts it under the target identity.
6. Returns a migration summary when all candidates succeed.
7. If `--prune` was requested and all candidates succeeded, deletes every
   non-target vector identity and records the deleted-row count.

If a provider does not declare an expected dimension, the first successful
candidate establishes it. An empty database can therefore complete a normal
no-op migration without a declared dimension, but `--prune` is rejected in
that case because there is no target identity to preserve.

The migration is fail-fast. A provider error, dimension mismatch, or storage
error stops the run and returns the existing typed error family. Vectors
written before the error remain staged, old vectors remain untouched, and no
pruning occurs. A subsequent invocation re-embeds the candidates and safely
replaces any target rows through the normal upsert path. Canonical records are
never part of the migration transaction and are never altered.

The service should expose a small summary value rather than make the CLI
calculate counts. The summary includes `model`, `dims`, `total`, `migrated`,
and `pruned`.

### Search and normal writes

After embedding a semantic query, search loads vectors using the provider
model and the query vector's exact dimension. Records with another model or
dimension are ignored. This makes an incomplete migration safe: search can
have partial coverage for the new identity, but it cannot compare incompatible
vectors.

After add/update/import paths that already perform best-effort embedding,
the vector is validated before upsert. Existing canonical-write semantics are
unchanged: provider/storage embedding failures are logged and do not roll back
the canonical operation.

## CLI integration

Add an `Embedding` command group to the Clap command enum and a `Migrate`
subcommand with `prune`, `yes`, and JSON output arguments. Route it through the
existing `run_cli` path so adapters do not contain SQL or migration logic.
The main binary builds the same configured `MemoryService` used by other local
commands. Rendering belongs in the CLI render module; the service returns the
typed summary.

The command's destructive confirmation applies only to `--prune`; generating
new derived vectors is not destructive. Refusing confirmation is an invalid
input and follows the existing exit-code contract.

## Compatibility and safety rules

- Existing old-model rows remain readable during and after schema upgrade.
- Old rows are never selected for a query whose model or dimension differs.
- A model change and a dimension change both create a distinct target vector
  identity.
- A failed migration leaves both old vectors and successful staged target
  rows. It never deletes vectors as a side effect of failure.
- Pruning removes only vector rows, never experiences.
- Lifecycle reconciliation and retention remain the source of truth for
  deleted/forgotten records; migration does not change their state.
- All command output follows existing HTML-free, JSON-safe rendering rules.

## Verification plan

Unit and integration tests will cover:

1. New vector-table creation uses the composite identity.
2. An old table is upgraded in place and existing vectors are preserved.
3. The same record can retain old and new dimensions simultaneously.
4. Exact model/dimension loading excludes mismatched vectors.
5. Migration includes active, inactive, and forgotten records, excludes
   deleted records, and leaves canonical records byte-for-byte unchanged.
6. A provider failure or dimension mismatch leaves old vectors and staged
   successes intact and does not prune.
7. A successful `--prune` removes only non-target identities and requires
   confirmation.
8. Normal semantic search does not score mismatched vectors.
9. CLI parsing, human output, JSON output, missing-provider errors, and
   confirmation behavior match the existing conventions.

Run the repository verification suite after implementation:

```text
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Documentation updates

Implementation will update:

- `docs/TODO.md` to mark the migration follow-up complete.
- `README.md` with the migration command and model/dimension behavior.
- `docs/cli.md` with syntax, output, confirmation, and retry semantics.
- `docs/storage.md` with the composite vector identity and lazy upgrade.
- `docs/architecture.md` with the service flow and failure guarantees.
