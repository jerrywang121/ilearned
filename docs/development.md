# Development

## Setup

```bash
cargo build
cargo test
```

Requires a C toolchain. rusqlite's `bundled-sqlcipher-vendored-openssl` feature
builds SQLCipher and OpenSSL from source, so normal builds do not require
system SQLCipher or OpenSSL headers. This increases native dependency build
time and artifact size; reqwest continues to use rustls.

## Verification (authoritative)

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

All four must pass before committing. Tests that spawn the binary
(`cli_test`, `rest_test`, `web_test`, `mcp_test`, `mcp_stdio_test`) build
the real binary via `CARGO_BIN_EXE_ilearned`; server tests bind ephemeral
loopback ports. MCP tests perform the full `initialize` →
`notifications/initialized` handshake (HTTP with session headers, stdio
with plain JSON-RPC lines).

## Notes

- HTTPS stack is rustls (`reqwest` with `default-features = false`). The
  vendored OpenSSL used by bundled SQLCipher is a build dependency, not the
  HTTPS runtime stack.
- Database encryption uses the shared key-aware opener; see the
  [configuration guide](config.md) for key handling. Keep `ILEARNED_DB_KEY` out
  of command arguments, test output, logs, fixtures, and generated TOML; use
  per-process environment injection in tests. No-key tests must continue
  to exercise plaintext compatibility, while migration tests cover
  `sqlcipher_export`, integrity verification, Unix `fs::rename`, Windows staged
  replacement, injected rollback failures, and stale `-wal`/`-shm` cleanup.
- Release archives and installers include `THIRD-PARTY-NOTICES.md`; keep its
  SQLCipher and OpenSSL notices aligned with the bundled versions in `Cargo.lock`.
- Askama templates are inline in `src/surfaces/http/web.rs`; there is no
  `templates/` directory to keep in sync.

## Release

Do not publish a release or push its tag without explicit author approval. A
push of a version tag starts the GitHub Actions release workflow.

The cargo-dist release matrix uses `macos-15` for Apple Silicon; macOS 14
GitHub-hosted runners were retired. With cargo-dist 0.32.0 installed as `dist`,
check that both macOS release targets use supported native runners with:

```bash
python3 tests/test_release_runners.py
```

1. Update the package version in `Cargo.toml` and run `cargo check` to refresh
   the matching `ilearned` entry in `Cargo.lock`.
2. Run all four verification commands above before committing.
3. Commit and push the version bump, then push the matching version tag:

   ```bash
   # Replace 0.2.5 with the version being released.
   git add Cargo.toml Cargo.lock
   git commit -m "chore: release 0.2.5"
   git push

   git tag v0.2.5
   git push origin v0.2.5
   ```
