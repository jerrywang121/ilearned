# SQLCipher Data-at-Rest Protection

- **Date:** 2026-10-04
- **Status:** Draft for written review
- **Scope:** Optional SQLCipher encryption for the local SQLite database and an explicit plaintext-to-encrypted migration command

## Context and goals

`ilearned` stores agents' learned experiences, feedback counters, lifecycle
metadata, FTS indexes, and optional embedding vectors in a local SQLite
database. The current storage is plaintext SQLite. This change adds opt-in
encryption at rest while preserving the existing local-first workflow for
users who do not configure a key.

The change will:

- compile SQLCipher into the application through
  `rusqlite`'s `bundled-sqlcipher-vendored-openssl` feature;
- read an optional database key only from the `ILEARNED_DB_KEY` environment
  variable;
- open newly created and existing databases with SQLCipher when that variable
  is set, while preserving plaintext SQLite behavior when it is absent;
- add `ilearned db encrypt` for an explicit, confirmation-gated conversion of
  an existing plaintext database;
- keep the key out of TOML files, configuration output, logs, command-line
  arguments, and JSON responses;
- preserve all existing application-service, repository, lifecycle, search,
  and adapter boundaries.

## Decisions and non-goals

### Decisions

- Encryption is **optional**, not mandatory. No key means plaintext mode for
  backwards compatibility.
- `ILEARNED_DB_KEY` is the only supported key source. The key is not accepted
  as a CLI option because command-line arguments can be exposed by process
  listings and shell history.
- A present-but-empty or non-UTF-8 key is invalid input; it is never treated as
  an absent key.
- A key supplied for a plaintext database does not trigger implicit
  conversion. Startup fails with a database-key error that points to
  `ilearned db encrypt`.
- Plaintext-to-encrypted conversion is explicit and in-process. It uses
  SQLCipher's attached-database `sqlcipher_export` mechanism rather than
  `sqlite3_rekey`, which does not convert plaintext databases.
- The migration command uses the resolved configured/fallback database path,
  requires `ILEARNED_DB_KEY`, and follows the existing destructive-action
  confirmation model (`--yes` or an interactive confirmation).
- This change does not add automatic conversion, decryption, key rotation, or
  a second key store.

### Non-goals

- Encrypting exported JSONL files, embedding-provider traffic, config files,
  logs, or the operating system's environment storage.
- Changing the public `MemoryService` operations or any REST, web, or MCP
  record contract.
- Introducing a system SQLCipher dependency or requiring system OpenSSL
  headers for normal builds.

## Alternatives considered

### Bundled SQLCipher with vendored OpenSSL — selected

`rusqlite 0.31` supports `bundled-sqlcipher-vendored-openssl`. This keeps one
portable SQLCipher implementation in all builds, retains the project's
existing no-system-OpenSSL-header expectation, and makes encrypted behavior
available to CLI, server, and MCP through the existing shared opener. The
trade-off is a larger/slower native dependency build and the bundled
SQLCipher/OpenSSL licensing documentation requirement.

### System SQLCipher

Linking against an installed SQLCipher would reduce bundled build work, but it
would make local builds, CI, and release artifacts depend on platform-specific
native libraries and crypto configuration. It conflicts with the current
portable bundled-SQLite approach.

### External `sqlcipher` conversion tool

Shelling out to an installed command would avoid implementing export logic in
Rust, but would not be portable or reliably testable. Runtime connections
would still need SQLCipher, so this adds operational complexity without
removing the main dependency change.

## Runtime architecture

### Configuration and key handling

Runtime configuration resolution will obtain `ILEARNED_DB_KEY` using the same
environment validation conventions as other environment settings. The value
will be held only for the current process and passed to database initialization
as an optional key. It will not be added to `FileConfig`, TOML generation, or
`ResolvedConfig`, so `config show` cannot display it accidentally.

The existing precedence and database-path behavior remain unchanged:

1. explicit database path/config selection;
2. `ILEARNED_DB` and config-file database settings;
3. existing project-local/XDG fallback databases;
4. startup failure when no path can be resolved.

`ILEARNED_DB_KEY` controls encryption independently of database-path
resolution. An explicitly configured missing path may still be created by
normal startup; when a key is present, that new file is encrypted from its
first write.

### Connection initialization

The SQLite opening seam in `storage::open_db` will accept
`key: Option<&str>`. `SqliteRepo` will pass that value through the existing
startup path, while the migration helper will call the same opener explicitly
with `None` for its plaintext source and `Some(key)` for verification.
Initialization will:

1. create the parent directory when normal startup is allowed to create it;
2. open the database with the bundled SQLCipher-backed `rusqlite` connection;
3. if a key is present, immediately issue SQLCipher's key pragma before any
   migration, `sqlite_master` read, WAL setup, or other schema access;
4. enable `PRAGMA cipher_memory_security = ON` for keyed connections;
5. apply the existing WAL, busy-timeout, and schema migration setup;
6. verify the expected FTS/schema objects as it does today.

When no key is present, the key pragma is not issued and the existing
plaintext-compatible initialization path is retained. SQLCipher's default
version-4 cipher settings are used; this change does not introduce custom page
sizes, KDF iteration settings, or compatibility modes.

The `MemoryService`, repository traits, lifecycle reconciliation, FTS queries,
embedding persistence, and all surface adapters continue to operate through
the same connection/repository path. No adapter gains SQL or key-management
logic.

### Key and database errors

The storage layer will recognize a failure during keyed initialization as a
database-key error rather than exposing the key or relying on a misleading
generic migration message. This covers a wrong key and attempting to open a
plaintext database with a key. The user-facing message will explain that the
database may need `ilearned db encrypt`; it will not include the key.

The CLI maps key/storage failures to the existing internal/storage exit-code
contract (exit 4). Invalid empty/non-UTF-8 environment values remain invalid
input (exit 2). HTTP, web, and MCP startup behavior continues to report the
shared initialization failure through their existing startup path.

## Plaintext-to-encrypted migration

### Command contract

```text
ilearned db encrypt [--yes] [--json]
```

The command is handled before building a normal keyed `MemoryService`, because
the source database must initially be opened without a key. It:

- resolves the database path using normal configuration rules;
- requires a non-empty `ILEARNED_DB_KEY`;
- requires an existing regular database file and never creates an empty source
  as a side effect;
- uses the standard confirmation behavior for destructive/replacement actions;
- reports only the result/path, never the key.

### Conversion flow

1. Inspect the source header and require the standard SQLite plaintext header.
   Already-encrypted, corrupt, or unsupported files are refused without
   modification.
2. Open the source in plaintext mode and validate the existing ilearned schema.
   Checkpoint/truncate any plaintext WAL state so data and sidecars are in a
   consistent state.
3. Create a temporary destination in the same directory/filesystem. Attach it
   with the requested SQLCipher key and run `sqlcipher_export` so the complete
   SQLite schema and contents are copied, including experiences, FTS tables,
   triggers, schema metadata, and lazily-created embedding rows.
4. Close the source and destination, open the temporary destination with the
   key, run SQLCipher integrity and ilearned schema checks, and close it.
5. Replace the source only after verification succeeds using a
   same-filesystem, platform-appropriate atomic/rollback-safe replacement.
   On Unix, copy the source mode bits to the temporary file before the
   replacement; on other platforms retain the temporary file's owner-only
   default permissions.
6. Remove stale plaintext `-wal`/`-shm` sidecars after successful replacement
   and clean up the temporary path. Failure before replacement leaves the
   original database usable and does not report success.

The operation does not retain a plaintext backup by default. Users who need a
portable backup should use the existing JSONL export before conversion; that
export is intentionally outside the database encryption boundary.

### Output and repeat behavior

Successful JSON output will identify the encrypted database path/result without
including key material. Human output will use the existing concise CLI style.
Confirmation refusal or missing/invalid key leaves the source unchanged.

Running the command against an already encrypted or unsupported file refuses
the operation instead of trying to rewrite it. Normal keyed startup can then
open the converted file; normal startup without a key continues to use
plaintext files only.

## Testing and acceptance

### Storage and migration tests

- A no-key database opens, migrates, writes, and remains readable as plaintext.
- A keyed new database can be reopened with the same key and has no plaintext
  SQLite header.
- A keyed database rejects a wrong key and a no-key open cannot read it.
- A plaintext database rejects keyed normal startup with the documented
  migration guidance.
- Existing schema migrations, FTS synchronization/rebuild, and embedding rows
  remain intact after conversion.
- Conversion copies the full database, verifies the encrypted destination,
  removes stale sidecars, and leaves the source unchanged when export or
  verification fails.
- Already-encrypted and unsupported source files are refused without mutation.

### CLI/config tests

- `ILEARNED_DB_KEY` reaches all normal CLI/server/MCP initialization paths.
- Empty and non-UTF-8 key values fail validation.
- `db encrypt` requires the key and confirmation, supports `--yes` and JSON
  output, and never prints the key.
- Keyed CRUD/search commands work after conversion and wrong-key startup fails
  with the expected exit code/message.
- `config show` works without opening a database and never exposes the key.
- Existing no-key integration suites continue to pass unchanged in behavior.

### Verification commands

The branch must pass:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Documentation and affected areas

Implementation is expected to touch the dependency lock/build configuration,
runtime config/startup, SQLite storage initialization and migration helpers,
CLI command parsing/dispatch/rendering, and focused storage/CLI tests.

Update these documents with the externally visible behavior:

- `README.md`: optional encryption, `ILEARNED_DB_KEY`, and conversion quick
  reference;
- `docs/architecture.md`: key-aware shared storage initialization;
- `docs/storage.md`: SQLCipher mode, key handling, migration, and sidecar
  behavior;
- `docs/cli.md`: `db encrypt`, confirmation, errors, and security rules;
- `docs/development.md`: bundled SQLCipher/vendored OpenSSL build notes;
- `AGENTS.md` or related agent guidance: the new key/migration invariant where
  it affects future changes.

No sample configuration file will contain a database key.
