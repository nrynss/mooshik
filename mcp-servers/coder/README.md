# Mooshik coder MCP server

A **stdio MCP server** that allows Mooshik to delegate code modifications to
external coding contractors and monitor their progress. Two tools, designed for
non-blocking execution and safe process management.

Rather than running heavy coding agents directly in-process or blocking the
companion conversation loop, this server spawns coding agents as child
processes, returns a tracking handle immediately, and lets Mooshik's workspace
watcher derive changes ambiently as files are modified on disk.

## What it exposes

| Tool | Arguments | Returns |
| --- | --- | --- |
| `delegate` | `task` (required), `repo` (required) | JSON string with handle, agent, repo, and task (`{"handle": "...", "agent": "...", "repo": "...", "task": "..."}`) |
| `check` | `handle` (required) | JSON string with handle and execution status (`{"handle": "...", "status": "running" \| "exited" \| "unknown", "exit_code": ...}`) |

Two tools, not six, on purpose. Mooshik keeps the companion's whole tool surface
to roughly eight tools so a small local model routes reliably (`dev-diary/PLAN.md`,
M10). These descriptions target that reader, explaining *when to reach
for the tool*, not how it is implemented internally.

**Non-blocking delegation.** Coding tasks can take minutes, while Mooshik's MCP
host imposes a hard 60s per-call timeout (`MCP_CALL_WAIT`). `delegate` writes
standing constraints, spawns the agent asynchronously, and returns a tracking
handle within milliseconds. The companion polls status with `check` as needed.

**Ambient derivation.** The contractor edits the repository filesystem directly.
Mooshik's workspace watcher observes the edits landing on disk and derives them
into the concept graph while the agent is still working. No large diffs or file
blobs need to be marshalled across the JSON-RPC wire.

## Supported Coding Agents

The server supports four external coding agents, selected with `--agent`:

| Agent | Binary & Invocations | Required Credentials / Env |
| --- | --- | --- |
| `claude` | `claude -p "..." --cwd <repo> --allowedTools "Edit,Write,Bash" --output-format stream-json` | `ANTHROPIC_API_KEY` |
| `omp` | `gemini -p "..." --sandbox=NONE --cwd <repo>` | `MOOSHIK_GEMINI_API_KEY` or `MOOSHIK_GEMINI_PROJECT` |
| `cursor` | `cursor-agent --task "..." --dir <repo>` | `CURSOR_API_KEY` |
| `agy` | `agy -p "..." --dangerously-skip-permissions` | `MOOSHIK_GEMINI_API_KEY` or `MOOSHIK_GEMINI_PROJECT` |

## Configuring Mooshik to spawn it

In `~/.mooshik/config.toml`:

```toml
[mcp_servers.coder]
command = "python3"
args = ["/absolute/path/to/mcp-servers/coder/server.py", "--agent", "claude"]
expose = ["delegate", "check"]   # empty list = never spawned

[mcp_servers.coder.env]
# The KEY is the environment variable the server reads.
# The VALUE is a vault secret NAME — never a literal token. Mooshik resolves
# every value here through the vault (`mcp_host::resolve_env`), so a literal
# would be looked up as a secret of that name, not found, and the server would
# refuse to spawn. That is exactly why `--agent` is an argument: the agent name
# is not a secret and has no vault entry to point at.
ANTHROPIC_API_KEY = "anthropic-api-key"
```

You can also configure the server automatically with the CLI:

```bash
mooshik configure coder --agent claude
```

`expose` is an allowlist and fail-closed: a server that exposes nothing is never
spawned, and tools absent from the list are rejected even if offered by the server.
Mooshik spawns lazily. The first `specs()` or `execute()` call starts the child.

`env` values for API keys are **vault secret names**, not literal tokens. Mooshik
resolves each one through the encrypted local vault at spawn time and injects the
resulting token into the child's environment. Set the vault secret with:

```bash
mooshik secret set anthropic-api-key
```

### The permission grant

Mooshik defaults to denying tool execution unless explicitly granted. Because
coding agents modify files on the local filesystem, permissions for coder tools
must be set to `prompt` rather than `allow` to require operator approval before
delegation:

```toml
[permissions]
"mcp.coder.*" = "prompt"
```

## Environment

All configuration is environment-only. No configuration file is read directly by
the server. **`--agent` is the only argument accepted**, and no *secret* is
ever accepted as one. A secret in `argv` shows up in `ps` listings and shell
history, so credentials come from the environment and only from there. With
neither `--agent` nor `MOOSHIK_CODER_AGENT`, or with an unknown agent name, the
server exits `2` with an explanatory message on stderr.

| Variable | Default | Meaning |
| --- | --- | --- |
| `MOOSHIK_CODER_AGENT` | (none) | Coding agent to delegate to: `claude`, `omp`, `cursor`, or `agy`. A fallback for a direct invocation. Under `[mcp_servers.coder]` pass `--agent` in `args` instead, because Mooshik reads every value in that `env` table as a vault secret name |
| `ANTHROPIC_API_KEY` | (none) | Anthropic API key, used when the agent is `claude` |
| `MOOSHIK_GEMINI_API_KEY` | (none) | Gemini Developer API key, used when the agent is `omp` or `agy` |
| `MOOSHIK_GEMINI_PROJECT` | (none) | Vertex AI project id, used when the agent is `omp` or `agy` |
| `CURSOR_API_KEY` | (none) | Cursor Agent API key, used when the agent is `cursor` |
| `CODER_TIMEOUT_SECS` | `10.0` | Per-call timeout for tool response handling |
| `CODER_LOG_LEVEL` | `INFO` | Level for stderr logging (`DEBUG`, `INFO`, `WARNING`, `ERROR`) |

## Design notes

**Standing rules via `AGENTS.md`.** Before spawning an agent, `delegate` writes
or refreshes an `AGENTS.md` file in the root of the target repository. The task
prompt instructs the agent to read `AGENTS.md` before making any changes. This
ensures the contractor consults Lambo memory via MCP for workspace constraints.

**Daemon lifecycle boundary.** The MCP server is a child of Mooshik, and agent
subprocesses are children of the coder server. On Linux, subprocesses are
spawned with `PR_SET_PDEATHSIG = SIGTERM` and isolated stdio
(`stdin=subprocess.DEVNULL`, `stdout=subprocess.DEVNULL`, `stderr=subprocess.DEVNULL`).
When Mooshik exits or the MCP server terminates, all active agent processes are
terminated immediately so no orphan processes continue editing repositories
unattended.

**stdout is the wire.** Under stdio transport, stdout carries JSON-RPC frames.
All logging is directed strictly to stderr (which Mooshik inherits into the
operator terminal). Stray prints to stdout are prevented to avoid corrupting
JSON-RPC framing.

**Failure containment & egress redaction.** Every tool runs within an error
containment guard. Upstream failures or timeouts return structured messages
rather than panicking onto the wire. Any known credential values present in the
server environment are scrubbed and replaced with `[redacted]` before error
messages are returned.

## Tests

The suite is **offline**: no network calls, no API keys, and no installed agent
binaries required.

* `FakeProcess` simulates `subprocess.Popen` execution, exit codes, and process
  termination.
* `ScriptedBackend` and `tests/wire_server.py` test the full JSON-RPC lifecycle
  over a real stdio transport with `mcp.ClientSession`.

```bash
cd mcp-servers/coder
python3 -m venv .venv && . .venv/bin/activate
pip install ../../mooshik-common
pip install -e ".[dev]"
pytest -q
```

From the repository root:

```bash
pytest mcp-servers/coder/tests -q
```

## Running it by hand

```bash
export ANTHROPIC_API_KEY=your-api-key
python3 mcp-servers/coder/server.py --agent claude   # MCP JSON-RPC on stdin/stdout

# MOOSHIK_CODER_AGENT still works for a direct run, if you prefer it:
#   MOOSHIK_CODER_AGENT=claude python3 mcp-servers/coder/server.py
```

Like all stdio MCP servers, it awaits JSON-RPC frames on stdin. Drive it with
an MCP client or let Mooshik manage its lifecycle automatically.
