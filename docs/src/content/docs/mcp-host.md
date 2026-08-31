---
title: MCP Client Host
description: Learn how Mooshik aggregates, isolates, and gates Model Context Protocol tools over stdio.
---

Mooshik acts as a native Model Context Protocol (MCP) host. It connects external tool servers over standard input and output streams (`stdio`), surfacing tools to the companion model under strict security gates.

## Stdio Child Process Architecture

Mooshik communicates with MCP servers through child processes:

- **Lazy spawning:** Mooshik spawns server processes only when a session requests tool specifications or execution. Sessions that do not use a server incur zero process overhead.
- **Framing safety:** Stdio transport uses stdout strictly for JSON-RPC message frames. Server logging is directed to stderr, which Mooshik displays in the terminal without corrupting wire protocols.
- **Process isolation:** Subprocesses run in isolated environments with allowlisted environment variables. Vault secrets are resolved at launch time and injected directly into the child process.

## Tool Namespacing

Tools discovered from connected servers are namespaced using the server identifier:

```text
mcp.<server_name>.<tool_name>
```

For example, the `search_news` tool provided by the `news` server becomes `mcp.news.search_news`.

## Configuring MCP Servers

Define servers in `~/.mooshik/config.toml`:

```toml
[mcp_servers.news]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]

[mcp_servers.news.env]
MOOSHIK_GEMINI_API_KEY = "gemini-api-key"
```

### Configuration Rules

1. **`expose` is an allowlist:** Only tools explicitly listed in `expose` are visible to the companion. If `expose` is empty or omitted, Mooshik never spawns the server.
2. **`env` values are vault secret names:** In the `[mcp_servers.<name>.env]` table, the key is the environment variable name the server expects, and the value is the secret name in the local encrypted vault. Mooshik resolves the secret value at spawn time.

## Permission Gating and Timeouts

All MCP tools are subject to permission policies configured under `[permissions]`:

```toml
[permissions]
"mcp.news.*" = "allow"
"mcp.coder.*" = "prompt"
```

- `"allow"`: The tool runs automatically when invoked by the companion.
- `"prompt"`: Mooshik prompts the operator in the terminal before running the tool.
- `"deny"`: Invocations fail immediately.

Mooshik enforces a hard 60-second per-call execution timeout (`MCP_CALL_WAIT`). Tools that hang or fail to respond within 60 seconds are terminated to protect companion loop responsiveness.
