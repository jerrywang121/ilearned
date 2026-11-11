# Install ilearned and set up MCP

## Install ilearned

Install ilearned if not yet installed

```bash
npm install -g --allow-scripts=@jerrywang121/ilearned @jerrywang121/ilearned
```

Verify ilearned is installed

```bash
ilearned --version
```

With the user's permission, choose workspace-local storage or user-global storage.
For workspace-local storage, run this from the workspace that should own the
memory:

```bash
ilearned config init
```

For user-global storage shared across workspaces, run:

```bash
ilearned config init -g
```

If the selected config file already exists, `config init` fails rather than overwriting
it; never use `--force` without separate approval.

After initialization, run `ilearned config show` from the same working directory
and environment that will launch the MCP process. If the
effective path is unexpected, stop and ask before changing configuration,
environment variables, or moving data.

## Setup MCP

Register ilearned as a **stdio** MCP server. Inspect an existing `ilearned`
entry first; preserve other servers and ask before replacing or changing its
database settings.

For OpenClaw, use its native MCP registry. Set `--cwd` to the workspace that used
for `config init`:

```bash
openclaw mcp add ilearned \
  --command ilearned \
  --arg mcp \
  --cwd /absolute/path/to/workspace
```

Verify mcp is working `openclaw mcp doctor ilearned --probe`.
You may need restart gateway `openclaw gateway restart`.

Other clients use different config-file locations and formats.
Below example for VS Code, Claude:

```json
{
  "mcpServers": {
    "ilearned": {
      "command": "ilearned",
      "args": ["mcp"]
    }
  }
}
```

Reload the active client and confirm live tool discovery.
If the active client cannot be checked, say verification remains incomplete.
