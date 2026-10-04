# Embedding Model and Dimension Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a safe, explicit CLI migration that rebuilds semantic vectors for the configured embedding provider while preserving canonical experiences and old vectors until optional confirmed pruning.

**Architecture:** Extend the embedding provider contract with an optional expected dimension and make `(model, dims)` the vector identity. `MemoryService::migrate_embeddings` will reconcile lifecycle state, re-embed every non-deleted experience through the existing provider boundary, stage each vector, and prune obsolete identities only after a complete successful run. SQLite will lazily rebuild an existing old-schema vector table, while CLI code will only parse, confirm, call the service, and render its summary.

**Tech Stack:** Rust 2021, SQLite/rusqlite 0.31, Clap 4, async-trait providers bridged through the existing Tokio helper, serde/serde_json, tempfile integration tests.

**Spec:** `docs/superpowers/specs/2026-10-04-embedding-migration-design.md`

## Global Constraints

- All behavior remains behind `application::MemoryService`; CLI and other adapters contain no SQL, lifecycle transitions, or ranking logic.
- Vector identity is `(topic, id, model, dims)`; semantic search loads only the exact active model and dimension.
- Migration processes every `state != 'deleted'` experience, including inactive and forgotten records, and never changes canonical experience fields or lifecycle state.
- A failed migration retains old vectors and any successful staged target vectors and never prunes; rerunning is the retry path.
- `--prune` is explicit, confirmation-gated, and runs only after every target embedding and upsert succeeds.
- Provider dimension mismatches are typed embedding failures; cosine similarity must not truncate mismatched vectors.
- Existing best-effort add/update/import embedding behavior remains non-transactional with respect to the canonical write.
- The vector table remains lazy and has no foreign key to `experiences`; lifecycle purge must continue removing all vector identities for expired records.
- No REST or MCP migration endpoint is added.
- All command output remains JSON-safe and follows the existing exit-code contract: embedding failures exit 3, invalid input/confirmation exits 2, storage/internal failures exit 4.
- Behavior changes update the matching documentation and tests.

## Review Focus

- **Old vector schema and data preservation:** an existing `(topic,id,model)` table must upgrade without losing rows; pinned by `old_embedding_table_is_upgraded_without_losing_vectors` in Task 2.
- **Malformed or inconsistent provider dimensions:** declared mismatches, empty vectors, and undeclared providers that change dimensions must fail before unsafe comparison; pinned by `cosine_rejects_mismatched_dimensions` in Task 1 and migration validation tests in Task 3.
- **Lifecycle candidate selection:** active, inactive, and forgotten records migrate while deleted records do not; pinned by `migration_reembeds_non_deleted_records_and_preserves_canonical_data` in Task 3.
- **Partial failure and destructive ordering:** staged rows and old rows survive a failure and pruning never runs; pinned by `migration_failure_preserves_old_vectors_and_skips_prune` in Task 3.
- **Operator-facing safety:** missing providers, JSON summaries, and refusal of `--prune` follow existing CLI behavior; pinned by the CLI parser/rendering and process tests in Task 4.

## File Map

Files are grouped by responsibility; no new runtime dependency or new adapter layer is planned.

- **Provider contract and ranking:** modify `src/embedding/provider.rs`, `src/embedding/openai.rs`, `src/embedding/fake.rs`, and `src/application/ranking.rs`.
- **Vector persistence:** modify `src/storage/embeddings.rs`; extend vector tests in `tests/storage_test.rs`.
- **Repository/application behavior:** modify `src/storage/repository.rs`, `src/application/service.rs`, and `src/application/mod.rs`; extend `tests/application_test.rs` and `tests/storage_test.rs`.
- **CLI:** modify `src/surfaces/cli/commands.rs`, `src/surfaces/cli/mod.rs`, `src/surfaces/cli/render.rs`, and `src/main.rs`; extend `tests/cli_test.rs`.
- **Documentation:** modify `README.md`, `docs/TODO.md`, `docs/cli.md`, `docs/storage.md`, and `docs/architecture.md`.

---

### Task 1: Add Dimension-Aware Provider Validation and Safe Cosine

**Files:**
- Modify: `src/embedding/provider.rs:EmbeddingProvider`
- Modify: `src/embedding/openai.rs:OpenAiEmbeddingProvider`
- Modify: `src/embedding/fake.rs:FakeEmbeddingProvider, FailingEmbeddingProvider`
- Modify: `src/application/ranking.rs:cosine`
- Test: `src/application/ranking.rs` unit tests

**Interfaces:**
- Consumes: existing `EmbeddingProvider::embed` and `model_id` contract.
- Produces: `EmbeddingProvider::dimensions(&self) -> Option<usize>`, defaulting to `None`; OpenAI returns `Some(EmbeddingConfig::dims)`, fake returns `Some(64)`, and the failing provider remains a typed failure provider. `cosine(&[f32], &[f32]) -> f32` returns `0.0` for unequal lengths or degenerate vectors instead of truncating to the shorter input.

- [ ] **Step 1: Write the failing ranking tests** in `src/application/ranking.rs`.

  Add unit tests named `cosine_rejects_mismatched_dimensions` and `cosine_returns_zero_for_empty_vectors`. Assert that a two-element versus three-element input returns `0.0`, while equal non-degenerate inputs still retain their normal cosine behavior.

- [ ] **Step 2: Run the focused tests to verify the old behavior fails.**

  Run: `cargo test cosine_`

  Expected: the new mismatch test fails because the current implementation compares the common prefix.

- [ ] **Step 3: Implement the provider dimension contract and exact-length cosine.**

  Add the default `dimensions` method without changing the required methods for existing external provider implementations. Store `cfg.dims` in `OpenAiEmbeddingProvider` and return it; return `64` from `FakeEmbeddingProvider` and leave `FailingEmbeddingProvider` with the default. Change `cosine` to check `a.len() != b.len()` before iterating and retain the existing zero-denominator behavior.

- [ ] **Step 4: Run the focused tests and existing embedding tests.**

  Run: `cargo test cosine_`

  Expected: all cosine tests pass, including equal-length ranking behavior.

- [ ] **Step 5: Commit the provider/ranking change.**

  ```bash
  git add src/embedding/provider.rs src/embedding/openai.rs src/embedding/fake.rs src/application/ranking.rs
  git commit -m "feat: validate embedding dimensions"
  ```

### Task 2: Upgrade Vector Storage to Composite Identity

**Files:**
- Modify: `src/storage/embeddings.rs:EMBEDDINGS_DDL, VectorStore, ensure_table`
- Modify: `src/application/service.rs:search_semantic` (call-site adaptation for the new exact-dimension load)
- Test: `tests/storage_test.rs`

**Interfaces:**
- Consumes: `EmbeddingProvider::dimensions` and exact-length `cosine` from Task 1.
- Produces: `VectorStore::upsert_vector(&self, topic: &str, id: &str, model: &str, v: &[f32]) -> Result<(), AppError>` with conflict identity `(topic,id,model,dims)`; `VectorStore::load_vectors(&self, topic: Option<&str>, model: &str, dims: usize) -> Result<Vec<(String, String, Vec<f32>)>, AppError>`; `VectorStore::delete_vectors(&self, topic: &str, id: &str) -> Result<(), AppError>` deleting every dimension; and `VectorStore::prune_vectors(&self, model: &str, dims: usize) -> Result<u64, AppError>` returning deleted non-target row count.

- [ ] **Step 1: Write failing storage tests for identity, upgrade, and pruning.**

  In `tests/storage_test.rs`, add:

  - `embedding_storage_uses_model_and_dimension_identity`: upsert the same `(topic,id,model)` with two vector lengths, assert both exact `(model,dims)` loads are present, and assert a wrong dimension load is empty.
  - `old_embedding_table_is_upgraded_without_losing_vectors`: manually create the old table with primary key `(topic,id,model)`, insert a vector, invoke a vector operation, and assert the row remains loadable under its stored dimension and the new schema accepts another dimension for the same record/model.
  - `prune_vectors_preserves_target_identity`: create target and obsolete model/dimension rows, call `prune_vectors`, assert the count and that only the target remains.

- [ ] **Step 2: Run the storage tests to verify they fail against the old schema/API.**

  Run: `cargo test --test storage_test embedding_storage_uses_model_and_dimension_identity`

  Expected: compilation or assertion failure because the current primary key and `load_vectors` API are model-only and there is no prune operation.

- [ ] **Step 3: Implement the composite-key DDL and lazy schema upgrade.**

  Change `EMBEDDINGS_DDL` to `PRIMARY KEY (topic,id,model,dims)`. In `ensure_table`, create the new table when absent; when an existing table has the old primary-key shape, rebuild it transactionally by creating a temporary new table, copying `topic,id,model,dims,vec`, dropping the old table, and renaming the replacement. Preserve the lazy creation behavior and do not add an `experiences` foreign key.

- [ ] **Step 4: Implement exact loading, all-identity deletion, and pruning.**

  Make `upsert_vector` conflict on all four key columns, make `load_vectors` filter on both `model` and `dims`, retain `delete_vectors` as a `(topic,id)` deletion across all identities, and implement `prune_vectors` with a single `DELETE` whose predicate preserves exactly the requested `(model,dims)` pair and returns the affected-row count.

- [ ] **Step 5: Adapt semantic search’s vector load call.**

  Pass `qv.len()` to `load_vectors` in `MemoryService::search_semantic`; this ensures the existing search path cannot receive vectors with a mismatched dimension before Task 3 adds the shared validation helper.

- [ ] **Step 6: Run storage, lifecycle, and semantic regression tests.**

  Run: `cargo test --test storage_test && cargo test --test application_test semantic_`

  Expected: all new schema tests and existing lifecycle/semantic tests pass, including purge behavior that deletes vectors through the unchanged `(topic,id)` cleanup path.

- [ ] **Step 7: Commit the storage migration.**

  ```bash
  git add src/storage/embeddings.rs src/application/service.rs tests/storage_test.rs
  git commit -m "feat: key embeddings by model and dimension"
  ```

### Task 3: Implement Service-Level Staged Migration

**Files:**
- Modify: `src/storage/repository.rs:ExperienceRepo, SqliteRepo`
- Modify: `src/application/service.rs:MemoryService`
- Modify: `src/application/mod.rs` export list
- Test: `tests/storage_test.rs` and `tests/application_test.rs`

**Interfaces:**
- Consumes: exact vector persistence from Task 2 and provider dimension contract from Task 1.
- Produces: `ExperienceRepo::list_embedding_candidates(&self) -> Result<Vec<Experience>, AppError>` selecting `state != 'deleted'` in deterministic order; `EmbeddingMigrationSummary { model: String, dims: Option<usize>, total: usize, migrated: usize, pruned: u64 }`; and `MemoryService::migrate_embeddings(&self, prune: bool) -> Result<EmbeddingMigrationSummary, AppError>`.

- [ ] **Step 1: Write failing repository/application migration tests.**

  Add a repository assertion that `list_embedding_candidates` returns active, inactive, and forgotten records but not deleted records. In `tests/application_test.rs`, add a small test-only provider implementing `EmbeddingProvider` with configurable model, optional expected dimensions, deterministic vectors, and an optional failure/call-count hook. Add these tests:

  - `migration_reembeds_non_deleted_records_and_preserves_canonical_data`: seed one record in each relevant state plus old-model vectors, run `migrate_embeddings(false)`, assert summary `total == migrated == 3`, target rows use the new model/dimension, old rows remain, deleted has no target row, and every canonical `Experience` is unchanged.
  - `migration_failure_preserves_old_vectors_and_skips_prune`: make a later provider call fail, call `migrate_embeddings(true)`, assert the typed error, old rows remain, earlier target rows remain staged, and obsolete rows were not pruned.
  - `migration_rejects_dimension_mismatch`: return a vector length different from a declared provider dimension and assert `AppError::EmbeddingUnavailable` with no invalid target row.
  - `migration_rejects_empty_vectors`: return an empty vector and assert `AppError::EmbeddingUnavailable` without storing it.
  - `migration_rejects_inconsistent_undeclared_dimensions`: return different lengths across candidates from a provider whose `dimensions()` is `None`; assert the first target row may remain staged but the run errors and does not prune.
  - `semantic_search_does_not_use_mismatched_vectors`: seed only a same-model vector with a different dimension from the query and assert semantic search returns no hit rather than scoring a prefix.

- [ ] **Step 2: Run the focused tests to confirm the service/repository interfaces are missing.**

  Run: `cargo test --test application_test migration_ -- --nocapture`

  Expected: compilation failures for the new repository method and migration summary/service method, or failing assertions before implementation.

- [ ] **Step 3: Add the repository candidate query.**

  Extend `ExperienceRepo` with `list_embedding_candidates`; implement it in `SqliteRepo` using the existing `row_to_exp` mapping, `WHERE state != 'deleted'`, and stable `ORDER BY topic,id`. Do not apply `is_eligible`, because inactive and forgotten records are deliberately included for re-embedding.

- [ ] **Step 4: Add shared provider-vector validation in `MemoryService`.**

  Refactor the existing embedding path around a private checked helper that obtains the configured provider, calls `block_embed`, rejects empty vectors, and rejects a returned length that differs from `provider.dimensions()` when present. Use this helper in `best_effort_embed` and `search_semantic`; preserve logging/non-rollback behavior for best-effort writes and typed errors for semantic queries. Keep the model from `provider.model_id()` and pass the query vector length to exact `load_vectors`.

- [ ] **Step 5: Implement `migrate_embeddings`.**

  Reconcile once, acquire the configured provider/model, read `list_embedding_candidates`, and use the checked helper for each `doc_text` built from `when_text`, `if_text`, `do_text`, and `check_text`. Establish the run dimension from the provider declaration or first vector, reject later inconsistencies, upsert each successful vector, and increment `migrated` only after the upsert succeeds. On any provider/dimension/storage error, return immediately without calling `prune_vectors`. On success, return `AppError::InvalidInput("cannot prune embeddings without a target dimension")` when `prune=true` but no target dimension can be established; otherwise call `prune_vectors(model,dims)` and place its count in the summary. Export `EmbeddingMigrationSummary` through `src/application/mod.rs`.

- [ ] **Step 6: Run the migration and regression tests.**

  Run: `cargo test --test storage_test && cargo test --test application_test`

  Expected: candidate visibility, staged failure, dimension validation, exact semantic matching, and all pre-existing application tests pass.

- [ ] **Step 7: Commit the service migration.**

  ```bash
  git add src/storage/repository.rs src/application/service.rs src/application/mod.rs tests/storage_test.rs tests/application_test.rs
  git commit -m "feat: add staged embedding migration service"
  ```

### Task 4: Expose and Test the CLI Migration Command

**Files:**
- Modify: `src/surfaces/cli/commands.rs:Commands, Commands::json`
- Modify: `src/surfaces/cli/mod.rs:run_cli`
- Modify: `src/surfaces/cli/render.rs`
- Modify: `src/main.rs` local-command dispatch
- Test: `tests/cli_test.rs`

**Interfaces:**
- Consumes: `MemoryService::migrate_embeddings(prune: bool)` and `EmbeddingMigrationSummary` from Task 3.
- Produces: `Commands::Embedding(EmbeddingArgs)`, `EmbeddingCommands::Migrate(EmbeddingMigrateArgs)`, and `render_embedding_migration(&EmbeddingMigrationSummary, json: bool) -> String`.

- [ ] **Step 1: Write failing CLI parser/render/process tests.**

  Add:

  - `embedding_migrate_parses_prune_yes_json`: parse `embedding migrate --prune --yes --json` and assert all flags are set.
  - `embedding_migrate_json_renders_summary`: use `run_cli` with a `MemoryService<SqliteRepo>` and `FakeEmbeddingProvider`, assert JSON has `embedding_migration.model`, `dims`, `total`, `migrated`, and `pruned`.
  - `embedding_migrate_requires_provider`: invoke the binary against a configured temporary database without embedding settings, assert exit code 3 and the existing JSON error shape.
  - `embedding_migrate_refuses_prune_without_confirmation`: pipe `n` to a binary invocation with `--prune` and assert exit code 2; no service migration or vector deletion should occur.

- [ ] **Step 2: Run the focused CLI tests to verify the command is not yet defined.**

  Run: `cargo test --test cli_test embedding_migrate`

  Expected: parser/compile failures for the missing command types and renderer.

- [ ] **Step 3: Add the Clap command group and flags.**

  Add `Embedding(EmbeddingArgs)` to `Commands`, make `Commands::json` read the nested migration output flag, and define `EmbeddingCommands::Migrate(EmbeddingMigrateArgs)` with `--prune`, `--yes`, and flattened `JsonArgs`. Keep help text explicit that migration uses the configured provider and prune is destructive.

- [ ] **Step 4: Add summary rendering and route the service operation.**

  Implement `render_embedding_migration` with the stable `{"embedding_migration": ...}` JSON shape and concise human output (`dims=unknown` when the summary dimension is `None`). In `run_cli`, confirm only when `--prune` is set, call `svc.migrate_embeddings(a.prune)`, and render the returned summary. Confirmation refusal returns `AppError::InvalidInput`.

- [ ] **Step 5: Include the command in `main.rs` dispatch.**

  Add `Commands::Embedding(_)` to the local service-command match arm so it uses the normal resolved config/database/provider setup and exits through the existing `render_error`/`exit_code` path.

- [ ] **Step 6: Run CLI and full application tests.**

  Run: `cargo test --test cli_test && cargo test --test application_test`

  Expected: all new command tests pass, including exit code 3 for no provider and exit code 2 for refused pruning; existing CLI behavior remains unchanged.

- [ ] **Step 7: Commit the CLI surface.**

  ```bash
  git add src/surfaces/cli/commands.rs src/surfaces/cli/mod.rs src/surfaces/cli/render.rs src/main.rs tests/cli_test.rs
  git commit -m "feat: add embedding migration CLI"
  ```

### Task 5: Reconcile User Documentation and TODO

**Files:**
- Modify: `README.md` embedding configuration/storage/follow-up sections
- Modify: `docs/TODO.md` follow-up checklist
- Modify: `docs/cli.md` command/config/output sections
- Modify: `docs/storage.md` embeddings schema and lazy upgrade behavior
- Modify: `docs/architecture.md` service/lifecycle/search invariants

**Interfaces:**
- Consumes: the shipped CLI syntax, summary shape, exact vector identity, and failure semantics from Tasks 2–4.
- Produces: documentation that no longer describes model-only lookup, dimension truncation, or manual per-record updates as the only migration path.

- [ ] **Step 1: Write the documentation updates.**

  Document `ilearned embedding migrate [--prune] [--yes] [--json]`, the configured provider requirement, model/dimension identity, inclusion of inactive/forgotten records and exclusion of deleted records, staged failure/retry semantics, and confirmation-gated pruning. Update the storage schema to `(topic,id,model,dims)` and describe the old-table lazy rebuild. Mark the follow-up in `docs/TODO.md` complete.

- [ ] **Step 2: Check documentation for contradictions and formatting.**

  Run: `git diff --check`

  Expected: no whitespace errors; grep the edited docs for the obsolete claims `(topic,id,model)`, “dims ... unused for filtering”, and “no automatic migration path” and remove or qualify each remaining occurrence.

- [ ] **Step 3: Commit the documentation reconciliation.**

  ```bash
  git add README.md docs/TODO.md docs/cli.md docs/storage.md docs/architecture.md
  git commit -m "docs: document embedding migration workflow"
  ```

### Task 6: Run Full Verification and Review the Branch

**Files:**
- Verify all files changed by Tasks 1–5; no new implementation files are expected.

**Interfaces:**
- Consumes: the complete implementation and documentation from prior tasks.
- Produces: a clean, verified `feat/embedding-migration` branch ready for code review.

- [ ] **Step 1: Format-check the branch.**

  Run: `cargo fmt --check`

  Expected: exit 0 with no formatting differences.

- [ ] **Step 2: Compile all targets.**

  Run: `cargo check`

  Expected: exit 0.

- [ ] **Step 3: Run the full test suite.**

  Run: `cargo test`

  Expected: all tests pass, including the migration/storage/CLI additions and all existing suites.

- [ ] **Step 4: Run strict linting.**

  Run: `cargo clippy --all-targets --all-features -- -D warnings`

  Expected: exit 0 with no warnings promoted to errors.

- [ ] **Step 5: Inspect the final diff and status.**

  Run: `git diff main...HEAD --check` and `git status --short`

  Expected: no whitespace errors, only intentional migration implementation/docs commits, and no untracked build artifacts.

- [ ] **Step 6: Request final code review before integration.**

  Use the project’s code-review workflow to verify service-boundary, schema-upgrade, staged-failure, and CLI confirmation invariants before considering the branch ready to merge.
