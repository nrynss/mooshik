---
title: MCP Host Architecture
description: Learn how Mooshik aggregates and gates Model Context Protocol tools.
---

Mooshik acts as a native Model Context Protocol (MCP) host. It aggregates external tool servers into a single interface for the companion model.

## Child Process Stdio Transport

Mooshik connects to MCP servers over standard input and output channels:
- It launches each server as a child process.
- It exchanges JSON-RPC messages across stdio.
- It captures stderr logs for operator debugging without corrupting JSON-RPC framing.

## Tool Discovery and Namespacing

During initialization, Mooshik queries connected servers using `tools/list`.

It namespaces tools using the server name:

```
mcp.<server_name>.<tool_name>
```

For example, the search tool in the `news` server becomes `mcp.news.search_news`.

## Permission Gating

Mooshik protects your environment with explicit permission rules:

- **Allow**: Tools execute immediately without prompts.
- **Prompt**: Mooshik requests confirmation before running the tool.
- **Deny**: Tool calls fail immediately.

Configure grants in `~/.mooshik/config.toml` or manage them via the CLI:

```sh
mooshik permissions
```
