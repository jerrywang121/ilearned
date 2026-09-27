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
