# AGENTS.md

Project: ilearned — local-first AI agent memory management. Read `README.md`
first, then `docs/architecture.md` and the spec at
`docs/superpowers/specs/2026-09-27-initial-architecture-design.md`.

## Boundary

All behavior lives in `application::MemoryService` — the only entry point
for adapters (`surfaces/cli`, `surfaces/http`). Adapters contain no SQL,
lifecycle transitions, or ranking logic.

## Invariants

- Compound key `(topic, id)`; `add` sets `good_count=1, bad_count=0,
  state=active`; `update` requires ≥1 non-blank field (blank-only values
  are ignored).
- `delete` on a never-existing id is `NotFound`; deleting an
  already-deleted record is idempotent success.
- Explicit `(topic, id)` reads go through `MemoryService::get` (sole
  entry): reconcile-first, `deleted`/missing read as `NotFound`;
  `forgotten`/`inactive` stay reachable (search still hides them).
- Search excludes `deleted`/`forgotten` always; `inactive` only with
  `deep=true`.
- Topics hierarchical (`topic/sub/...`, segments `[a-z0-9_-]`; `#`
  pattern-only, never stored): writes validate canonical form,
  `search`/`export --topic` accept `#` multi-level wildcards (bare =
  exact only; `clear --topic` stays exact), `list_topics` lists/searches
  distinct topics.
- SQLite `when_text/if_text/do_text/check_text`, INTEGER epoch timestamps;
  wire uses `when/if/do/check`, RFC 3339.
- Reconcile before every op (strict `>`: 60d→inactive, 120d→forgotten);
  purge `deleted`/`forgotten` with retention start older than 60d.
- Embedding failure on write never rolls back; semantic query without an
  embedding fails typed (503), never silently text-only.
- Destructive actions need explicit confirmation; all HTML escaped.
- Database encryption is optional: an unset `ILEARNED_DB_KEY` preserves
  plaintext SQLite compatibility. A present-but-empty or non-UTF-8 value is
  invalid input.
- `ILEARNED_DB_KEY` is the only key source and is environment-only: never add
  it to TOML, generated config, CLI arguments, logs, errors, or output;
  `config show` must not reveal it.
- The shared SQLite opener applies a configured key before schema access and
  keyed startup errors map to storage/internal failure without exposing key
  material. A key never causes automatic conversion of a plaintext database.
- Plaintext conversion is explicit and confirmation-gated through
  `ilearned db encrypt [--yes] [--json]`, using `sqlcipher_export` and verified
  replacement; failures leave the source unchanged and clean temporary output.
  JSONL exports are not encrypted by this feature.

## Verification

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Behavior changes require updating the matching `docs/*.md` and tests.

## Release

DO NOT do Release without explicit confirmation from the author.
The Github Action has been setup for the build and release.
To trigger the release with new version, use below command. 

```bash
# 1. Edit Cargo.toml: version = "0.1.1"
# 2. Commit
git add Cargo.toml
git commit -m "chore: release 0.1.1"
git push

# 3. Tag and push the tag (triggers the workflow)
git tag v0.1.1
git push origin v0.1.1
```
