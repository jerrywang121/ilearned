# Development

## Setup

```bash
cargo build
cargo test
```

Requires a C toolchain (rusqlite `bundled` builds SQLite from source) and
no OpenSSL headers (reqwest uses rustls).

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

- HTTPS stack is rustls (`reqwest` with `default-features = false`), so no
  system OpenSSL dev headers are needed.
- Askama templates are inline in `src/surfaces/http/web.rs`; there is no
  `templates/` directory to keep in sync.
