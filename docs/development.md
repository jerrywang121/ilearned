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
- Database encryption uses the shared key-aware opener. Keep `ILEARNED_DB_KEY`
  out of command arguments, test output, logs, fixtures, and generated TOML;
  use per-process environment injection in tests. No-key tests must continue
  to exercise plaintext compatibility, while migration tests cover
  `sqlcipher_export`, integrity verification, atomic replacement, and stale
  `-wal`/`-shm` cleanup.
- Askama templates are inline in `src/surfaces/http/web.rs`; there is no
  `templates/` directory to keep in sync.
