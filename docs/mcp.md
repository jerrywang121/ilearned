# MCP

- Transport: streamable HTTP at `/mcp` (same listener as REST/web).
- Tools: `search`, `add`, `modify`, `delete`, `promote`, `downgrade`,
  `clear`. Schemas and result semantics match the application commands
  exactly — no MCP-specific business rules or alternate record shape.
- Typed errors propagate as MCP errors (invalid → invalid params,
  not-found → not found, embedding failures → internal with a clear
  message).
