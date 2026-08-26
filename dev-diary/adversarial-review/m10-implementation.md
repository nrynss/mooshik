# M10 — The MCP host implementation record

## Scope

M10 adds the MCP host: configured `[mcp_servers.*]` in `config.toml` bring in
external MCP servers as `mcp.<server>.<tool>` companion tools, gated by the
existing M5 permission system (`mcp.<server>.*` = "allow") and scanning the same
M6 egress redaction.

The tool surface grows from 4 hand-written tools to 4 + (configured MCP tools);
`search_web` and `fetch_page` return through configured servers rather than
hand-written Rust tools.

## Layout

| Path | Role |
|------|------|
| `src/mcp_host/mod.rs` | `McpTools` — the `ToolExecutor` that spawns and drives MCP children |
| `src/mcp_host/tests.rs` | 10 deterministic, net-free tests against a fixture stdio MCP server |
| `src/mcp_host/tests/fixture_server.py` | Fixture MCP server (Python stdlib, no external deps) |
| `src/config/mod.rs` | `McpServerConfig`, `Config::validate_mcp`, `[mcp_servers.*]` parsing, `DEFAULT_TOML` example |
| `src/tools/mod.rs` | `CompositeTools` composing memory + MCP behind the single gate/redaction chain |
| `src/text/en.toml` | `tools.mcp_*` string keys |

## Key decisions

### Spawn-if-expose (chosen)
Servers with an empty `expose` list are discarded at construction and **never**
spawned. Reason: an operator who writes a server entry but exposes nothing
likely intends to keep it inert (fail-closed). Spawning it but hiding all tools
would waste resources — the child sits there doing nothing, using memory and a
process slot, with no path to become useful short of a config reload.

### Lazy spawn (chosen)
Servers spawn on first `specs()` or `execute()` call, not at `executor_for_chat`
construction. Reason: chat startup is already bounded (memory open has a 20 s
timeout); adding N × 30 s per server would make it visibly slow. A companion
that never calls MCP tools pays no process-lifecycle overhead. The one-time
spawn cost shifts to the first tool call or the first turn that needs specs.

### Reconnect-on-crash, bounded (chosen)
If a child is dead when a tool is called, a single restart attempt is made.
If the respawned child still fails, the call returns a contained error string
and the live slot stays closed (next call may retry again). This bounds the
outage: a misbehaving server cannot cause infinite respawn loops.

### Vault-ref resolution at spawn (chosen)
The `[mcp_servers.<name>.env]` map names vault secrets by name (never value).
Resolution happens at spawn time, before the child starts. If the vault is
unavailable or a named secret is missing, that server fails closed: no tools
contributed, one stderr notice. Other servers are unaffected. This mirrors
the `[tools.scratch.env]` pattern (M6).

### Tool naming: `mcp.<server>.<tool>` (chosen)
The config key `server` is the map entry name, `<tool>` is the server's reported
tool name. The M5 permission prefix grammar `mcp.github.*` matches these with
the least surprising mapping. `specs()` returns only exposed tools (filtered by
the per-server allowlist); the gate further filters by the grant table.

### Composition: `CompositeTools` (chosen)
Instead of threading MCP tools into the existing `MemoryTools`, a tiny
`CompositeTools(inner, mcp)` wrapper dispatches by the `mcp.` prefix to the MCP
host, everything else to memory. Both sides sit behind the **same** `GatedTools`
and `RedactingTools`, so grants and egress redaction work identically for both.

### All async on the ToolRuntime worker (chosen)
`ToolExecutor::execute` is synchronous and called from the chat loop's own Tokio
runtime. All rmcp interaction (spawn, handshake, tool discovery, call-tool
rpc) happens on a dedicated worker thread via `rt.block_on`, mirroring the
existing `ToolRuntime` / `MemoryTools` pattern.

## Dependencies

`rmcp = { version = "3.1.2", default-features = false, features = ["client",
"transport-child-process"] }` — pinned `3.1.2` with `default-features = false`
so rmcp's optional reqwest (`^0.13.2`) never compiles alongside the repo's
`reqwest 0.12`.

## Test coverage (net-free)

All 10 tests use a Python stdlib fixture server spawned as a child process.
No network, no external services:

| Test | What it covers |
|------|---------------|
| `specs_expose_only_the_allowlisted_tools` | Expose filtering + schema/description from MCP Tool |
| `empty_expose_leaves_the_server_inert` | No expose → never spawned, empty specs |
| `execute_echo_round_trips_arguments` | Round-trip execute → JSON args echoed back |
| `execute_add_returns_the_sum` | Parameterized tool call |
| `unknown_tool_returns_a_contained_error` | Non-exposed tool / wrong server / bad name |
| `is_error_result_becomes_a_contained_error_string` | Server `isError: true` → model sees text |
| `a_crashed_child_is_reconnected_on_the_next_call` | Child exit → automatic respawn |
| `missing_secret_fails_the_server_closed` | Missing vault secret → server fails closed |
| `a_present_secret_is_injected_into_the_child_environment` | Vault ref resolves → server starts |
| `mcp_tools_are_absent_without_a_grant_and_present_with_one` | Gate integration (M5 path through `executor_for_chat`) |

## Config tests

4 config-layer tests cover parsing, validation (empty command, bad env ref), and
inert-expose semantics.

## What is NOT in this milestone

- **No SSE/HTTP transport.** M10 uses `transport-child-process` only; streamable
  HTTP client transport is trivial to add later but adds no new capability to the
  existing architecture.
- **No `mooshik mcp-list` subcommand.** The companion sees MCP tools through
  `specs()`; an operator-facing command is future work.
- **No live verification at commit time.** Performed by Main afterwards (below).

## Live verification (performed 2026-08-26, real Google endpoints)

Setup (all on this machine, real Cloud SQL + Vertex):
1. `[mcp_servers.memory]` → `/home/nryn/work/lambo/target/debug/lambo serve --session m10-live`,
   vault-ref env (`LAMBO_STORE`/`LAMBO_EMBEDDER`/`LAMBO_EMBED_DIM`/
   `LAMBO_POSTGRES_DSN`/`LAMBO_GEMINI_PROJECT`/`LAMBO_GEMINI_LOCATION`/
   `GCP_LAMBO_CREDENTIALS`, each a real vault secret), `expose` =
   `["lambo_derive","lambo_stats","lambo_recall"]`.
2. `[permissions] 'mcp.memory.*' = "allow"`.
3. `mooshik chat` turn: the model called `mcp.memory.lambo_derive` with content
   `m10 mcp host live marker via real lambo server`; the lambo-serve child
   logged session attach (`session=m10-live`, embedder=gemini dim=1536), the
   tfx returned `Concept stored for agent m10-live (receipt …)`, and the child
   closed cleanly on chat exit. One benign cold-start warning: the write-queue
   embedder **probe** timing out at 5s — the actual derive embedded fine.
4. `stats` (session m10-live): 1 concept, **1 embedded**, log depth 0.
5. Fresh-process `recall "m10 mcp host live marker"` (session m10-live, after
   the child's lease cleared): returns the concept verbatim (relevance 1.07).

Grant-denial is pinned offline (specs filtering by the M5 gate); the live run
demonstrates the allow path end to end through a real MCP server over Cloud SQL.