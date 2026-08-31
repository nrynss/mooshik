---
title: Coder Server
description: Delegate repository code changes to external coding agents via stdio MCP.
---

The `coder` server is a stdio MCP server that allows Mooshik to delegate code modifications to external coding CLIs and monitor their progress asynchronously.

## Asynchronous Non-Blocking Execution

Code modifications can take minutes to complete, exceeding Mooshik's 60-second host timeout.

The coder server uses an asynchronous execution model:
- **Immediate tracking handle:** `delegate` writes standing constraints, launches the external agent in the background, and returns a JSON tracking handle within milliseconds.
- **Liveness polling:** The companion or operator polls execution status using `check`.
- **Ambient change capture:** The external agent edits files on disk directly. The ambient workspace watcher detects file changes and records them into the memory graph while the agent works.

## Supported Coding Agents

The coder server supports four external coding CLIs via the `--agent` argument:

| Agent Identifier | Target Tool | Authentication Requirements |
| :--- | :--- | :--- |
| `claude` | Claude Code CLI (`claude`) | `ANTHROPIC_API_KEY` |
| `omp` | Gemini CLI (`gemini`) | `MOOSHIK_GEMINI_API_KEY` or `MOOSHIK_GEMINI_PROJECT` |
| `cursor` | Cursor Agent CLI (`cursor-agent`) | `CURSOR_API_KEY` |
| `agy` | Antigravity CLI (`agy`) | `MOOSHIK_GEMINI_API_KEY` or `MOOSHIK_GEMINI_PROJECT` |

The server contains no agent of its own. It shells out to the CLI installed and authenticated on your machine.

## Server Configuration

Configure the server automatically with the CLI:

```bash
mooshik configure coder --agent claude
```

This updates `~/.mooshik/config.toml`:

```toml
[mcp_servers.coder]
command = "/home/you/.local/share/mooshik/venv/bin/mooshik-coder-mcp"
args = ["--agent", "claude"]
expose = ["delegate", "check"]

[mcp_servers.coder.env]
ANTHROPIC_API_KEY = "anthropic-api-key"
```

### Permission Configuration

Because coding agents modify the filesystem, permissions must require operator confirmation:

```toml
[permissions]
"mcp.coder.*" = "prompt"
```

## Standing Constraints via `AGENTS.md`

Before spawning an agent, `delegate` writes or refreshes an `AGENTS.md` file in the root of the target repository.

The file includes relevant constraints and architecture rules retrieved from Lambo memory. The task prompt instructs the agent to read `AGENTS.md` before making edits, ensuring external tools adhere to established project conventions.

## Process Lifecycle Management

On Linux systems, child processes are spawned with `PR_SET_PDEATHSIG` set to `SIGTERM`. When Mooshik exits or the MCP server terminates, all active agent processes are stopped immediately to prevent orphaned background edits.
