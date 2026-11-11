# Installation

## npm

```bash
npm install -g --allow-scripts=@jerrywang121/ilearned @jerrywang121/ilearned
```

## Platform installers

Linux and macOS:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jerrywang121/ilearned/releases/download/v0.1.0/ilearned-installer.sh | sh
```

Windows:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/jerrywang121/ilearned/releases/download/v0.1.0/ilearned-installer.ps1 | iex"
```

These commands install the pinned `v0.1.0` release. See the
[releases page](https://github.com/jerrywang121/ilearned/releases) for other versions.

## Build from source

Requirements: Rust toolchain (1.98+) and a C toolchain. SQLite/SQLCipher and
vendored OpenSSL build from source through rusqlite; no system OpenSSL headers
are needed.

```bash
git clone https://github.com/jerrywang121/ilearned
cd ilearned
cargo build --release
./target/release/ilearned --help
```

Distributed binaries include bundled SQLCipher/OpenSSL components; see
[third-party notices](../THIRD-PARTY-NOTICES.md). After installation, follow the
[quickstart](../README.md#quickstart), use the [configuration guide](config.md)
for settings and database paths, and consult the [CLI guide](cli.md) for
commands.
