# ilearned

> Lightweight, local-first memory for self-improving AI agents — a live rule book of learned experiences and facts.

[![Build](https://img.shields.io/badge/build-cargo%20test-green)](docs/development.md)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.98%2B-orange)](Cargo.toml)

## What it is

`ilearned` stores lessons and facts as searchable **experiences** grouped by
topic. Each experience has a required action or fact (`do`) and optional context
(`when`, `if`, `check`). Agents can retrieve, apply, and refine those rules.

## Features

- **One binary, five surfaces:** CLI, REST, web UI, HTTP MCP, and stdio MCP.
- **Searchable rules:** keyword search works without a provider; optional
  embeddings add semantic search.
- **Lifecycle and feedback:** experiences age into inactive and forgotten states;
  promote and demote record outcomes. Demoting can automatically delete a rule.
- **Portable data:** import and export experiences as JSONL.
- **Local-first security:** databases are plaintext by default; optional SQLCipher
  encryption uses the environment-only `ILEARNED_DB_KEY`. Existing plaintext databases
  require explicit, confirmation-gated conversion. **JSONL exports remain plaintext**
  and need separate protection.

## Installation

```bash
npm install -g --allow-scripts=@jerrywang121/ilearned @jerrywang121/ilearned
```

See [installation options](docs/installation.md) for running with platform
installers, or building from source.

## Quickstart

In a new workspace, initialize a local configuration and database path:

```bash
ilearned config init
ilearned add --topic "travel/hotel/booking" --when "family trip" --if "child age < 1y old" \
  --do "request baby cot" --check "hotel website states baby cot available"
ilearned search --topic "travel/#" --text child --json
```

For initialization, existing configs, global storage, path resolution, and
overrides, see the [configuration guide](docs/config.md).
Run `ilearned serve` for the web UI, REST API, and HTTP MCP on
`127.0.0.1:8787`, or `ilearned mcp` for stdio MCP. The server has no
authentication; do not expose it to untrusted networks.

## Agent Skill

`ilearned` provides persistent memory; an agent skill defines when to learn,
which lessons to apply, and how to verify and refine them.

### Self-improving OpenClaw, LobeHub, and Hermes assistant

The [ilearned-self-improving-agent skill](./example/skills/ilearned-self-improving-agent/SKILL.md)
turns user corrections, tool failures, knowledge gaps, and verified better
approaches into structured rules for daily planning, research, communication,
and tool use. It searches and revalidates lessons before reuse, avoids duplicate
or speculative learning, and ties feedback to actual outcomes rather than reads
or successful memory writes.

The skill links to a separate [first-use installation and MCP setup
guide](./example/skills/ilearned-self-improving-agent/references/setup.md).
For advanced configuration, check [Configuration](docs/config.md) and
[MCP](docs/mcp.md)

Install with [Vercel's skills CLI](https://skills.sh/) for OpenClaw:

```bash
npx skills add jerrywang121/ilearned --skill ilearned-self-improving-agent --agent openclaw
```

Run the command from the OpenClaw workspace (or add `--global` for a shared
installation). It uses the repository's default branch, so it becomes available
there after these changes are merged and pushed. The repository also contains a
travel example; `--skill` selects only the self-improving skill.

Alternatively, for a local OpenClaw installation, copy the skill directory into
the active OpenClaw workspace's `skills/` directory:

```bash
cp -R example/skills/ilearned-self-improving-agent /path/to/openclaw-workspace/skills/
```

For **LobeHub**, open the agent's **Skills → Add Skill…**, choose **Add → Import
from GitHub**, and enter the directory URL once the branch is available:

```text
https://github.com/jerrywang121/ilearned/tree/main/example/skills/ilearned-self-improving-agent
```

For **Hermes Agent**, install from GitHub once the skill directory is available
on the repository's default branch:

```bash
hermes skills install jerrywang121/ilearned/example/skills/ilearned-self-improving-agent
```

Review the security scan. The installation and MCP setup guide is included in
the skill package; configure ilearned as a stdio MCP server in Hermes and verify
live tools in a new session.

If distributed through ClawHub, install the published skill package.

Installing a skill alone does not install the executable or configure MCP.
Ask the assistant:
**“Set up ilearned using this skill, verify the memory tools, and use it for
reusable lessons from my daily tasks.”**
Setup changes need permission; the skill does not automatically rewrite agent
instructions or share personal memories.

For a domain-specific example SKILL, see [ilearned-travel-agent](./example/skills/ilearned-travel-agent/SKILL.md).

## Documentation

- [Configuration](docs/config.md) · [CLI, import/export, and encryption](docs/cli.md)
- [REST API](docs/rest-api.md) · [Web UI](docs/web-server.md) · [MCP](docs/mcp.md)
- [Architecture](docs/architecture.md) · [Storage and lifecycle](docs/storage.md)
- [Development and verification](docs/development.md) · [Roadmap](docs/TODO.md)
- [Third-party notices](THIRD-PARTY-NOTICES.md)

## License

MIT — see [LICENSE](LICENSE).
