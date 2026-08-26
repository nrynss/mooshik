# M10 round 1 — adversarial review of the MCP host

Reviewed at `5877e66` (branch `m10-mcp-host`), worktree `/tmp/mooshik-m10`.
Scope: `Cargo.toml`/`Cargo.lock`, `src/config/mod.rs` + `overlay.rs`,
`src/mcp_host/mod.rs` + `tests.rs` + `tests/fixture_server.py`, `src/tools/mod.rs`
(`CompositeTools` + `executor_for_chat`), `src/tools/permissions.rs` / `redact.rs`,
`src/text/en.toml`, cross-checked against PLAN M10 and `docs/SPEC.md` companion
slot. Every claim below is executed or directly traced, not read off.

## Findings

### P1

None.

### P2

1. **A wedged-but-alive MCP child permanently wedges the shared tool worker
   thread (bounded-wait never actually bounds).** `execute_on_worker` wraps the
   spawns in `timeout(MCP_SPAWN_WAIT, …)` but the two `live.call_tool(...).await`
   sites (phase 2 and the phase-3 retry) are **not** wrapped in any inner
   timeout. The only deadline is the outer `worker.run(..., MCP_CALL_WAIT +
   MCP_SPAWN_WAIT = 90s)` budget, and `ToolRuntime::run` fires that on the
   *caller* thread via `recv_timeout` — the worker thread itself stays stuck in
   `rt.block_on(execute_on_worker)` awaiting an rmcp `call_tool` that a
   wedged-but-alive child never answers. Because `McpTools` owns **one**
   `ToolRuntime` (one `worker_threads(1)` runtime on the dedicated
   `mooshik-tools` thread), that one wedged RPC DoS-es every subsequent tool use
   on the same `McpTools` instance — the memory tools share this worker only in
   the sense that `CompositeTools` fans memory to `MemoryTools`'s *own* worker,
   so the blast radius is the MCP surface itself: each later MCP call blocks the
   caller the full 90 s and returns the contained `internal_error`, forever.
   The `MCP_CALL_WAIT` constant is documented as "the bound on one MCP tool call
   round-trip" — it is dead as an inner bound. Fix: wrap each `live.call_tool`
   in its own `tokio::time::timeout(MCP_CALL_WAIT, …)` so a non-answering child
   frees the worker thread after 60 s instead of pinning it indefinitely
   (the existing `contract` says "bounded by MCP_CALL_WAIT"; the code does not
   deliver that). Not currently pinned by a test.

### P3

1. **A config key whose *server name contains a dot* is unreachable.**
   `parse_mcp_name` splits on the **first** dot after `mcp.`: `mcp.github.app.x`
   parses to `server="github"`, `tool="app.x"`. If the config key is `github.app`
   (server name with a dot) there is no `github` server and the call returns the
   contained `tools.mcp_tool_unknown`/`unknown MCP server` error — the `github.app`
   server can never be addressed, and if a genuine `github` server also existed,
   `mcp.github.app.x` would dispatch to `github` with tool `app.x` rather than to
   `github.app`. The tool grammar `mcp.<server>.<tool>` with dot-containing server
   names is inherently ambiguous; the code resolves it silently to the
   wrong/absent server rather than failing config validation up front. A server
   name containing `.` should be rejected at `validate_mcp` (fail closed at load,
   matching the rest of the config philosophy), or the parse made unambiguous.
   Verified by an adversarial test (`mcp.github.app.x` → contains `unknown MCP
   server`, no crash). Currently unpinned.

2. **`ensure_spawned` marks spawned before lazy spawn completes, so a server
   that fails its first spawn is never retried at the specs/ensure layer — only
   the per-execute phase-1 respawn retries.** `ensure_spawned` sets
   `spawned=true` *before* `spawn_all` runs; a failed first spawn leaves the slot
   `None` and every subsequent `specs()` short-circuits, so the tool list never
   recovers even after the underlying transient (vault briefly down, child
   start race) clears. The execute path does recover via phase 1 (slot None →
   respawn). Minor, and the bounded respawn-on-execute makes it tolerable, but the
   cached `all_specs` is never refreshed after a recoverable initial failure, so
   `specs()` can permanently hide a server whose process now would come up.
   No test pins retry-on-later-spec.

## What held up under attack

* **Dependency ✓.** `Cargo.lock` contains exactly **one** `reqwest`
  (`reqwest 0.12.28` at line 2213). `rmcp` (resolved **3.1.4**, so the lock has
  moved past the `Cargo.toml`'s `3.1.2` semver — expected and harmless) has **no**
  `reqwest` in its dependency list: with `default-features = false` +
  `client` + `transport-child-process`, rmcp's optional `reqwest ^0.13.2` never
  came in. No second reqwest, no 0.13. Confirmed green build + `cargo test
  --locked`.
* **Config validation ✓.** `McpServerConfig` is `deny_unknown_fields` (unknown
  server keys fail load); `[mcp_servers.*]` is a `BTreeMap` so duplicate names
  are a TOML parse error. `env` map values are validated secret-name-shaped
  (`is_valid_name`) and keys as env-var names in `validate_mcp`, which
  `overlay.rs` calls so the whole table fails closed at load — an entry that
  could never spawn never reaches call time. `expose` empty is legal and inert.
* **Empty-expose truly inert ✓ — proven by a marker-env test.** `McpTools::
  from_config` discards empty-expose servers *before* allocating them slots;
  all spawn is driven through `lives[idx]`, so an empty-expose server has no slot
  and no code path can ever run its command. Wrote a one-off test pointing a
  `.env` value at a marker file whose creation would prove a spawn; `specs()`
  spawned nothing, marker never appeared, test passed. Reverted.
* **Cross-server same-name dispatch ✓.** Two servers `a` and `b` both exposing
  `echo` route `mcp.a.echo` and `mcp.b.echo` to their own processes (proven by an
  adversarial test; the fixture echoes args, so a misroute would surface).
  Disambiguation is exact config-key match — correct given `BTreeMap` key
  uniqueness.
* **isError does not trigger respawn ✓.** `CallError::ToolError` returns the
  server's text as the model-visible contained error and puts the live session
  back; only `CallError::Transport` drops and respawns. Traced: the phase-3
  branches split ToolError (no respawn) from Transport (one respawn) cleanly.
* **Slot lock never held across an await ✓.** Each phase locks, takes/sets,
  releases: phase 1 takes to decide, releases, spawns without the lock, then
  relocks to set; phase 2 `take()`s the live server out before the `call_tool`
  await; phase 3 sets the slot back after the await. No `MutexGuard` spans an
  `await`. Using `parking_lot::Mutex` (poison-free) is the right call here.
* **One respawn + one retry, bounded ✓.** Per execute call: phase-1 spawn (if
  slot None/dead) + phase-3 single respawn + single retry RPC. Each is bounded by
  `MCP_SPAWN_WAIT`/the outer budget. No unbounded respawn loop.
* **Missing secret / vault-unavailable fails only that server ✓.** `resolve_env`
  returns `Err` → `spawn_one` returns `None` → that slot stays closed, contained
  stderr notice; `spawn_all` iterates independently so other servers proceed.
  Pinned by `missing_secret_fails_the_server_closed`.
* **Composition: mcp.* cannot leak into the memory branch ✓.** `CompositeTools
  ::execute` routes purely on `name.starts_with("mcp.")`; memory tools are
  `lambo_*`/`run_scratch_script`, none of which start with `mcp.`. `specs()`
  unions inner + mcp (memory tool names cannot collide with `mcp.*` full names).
  An `mcp.`-prefixed name can never reach `MemoryTools`.
* **GatedTools wraps the composite — MCP cannot bypass the gate ✓.** `executor_for_chat`
  builds the composite then `compose_chat_stack` = `GatedTools(RedactingTools(composite))`.
  The gate's `specs()` filters `mcp.srv.*` by grant; `executor_for_chat(&default, None)`
  surfaces **zero** `mcp.` tools. Critical check: a config with no `mcp.*` grant
  exposes no MCP tools — pinned by `mcp_tools_are_absent_without_a_grant_and_present_with_one`.
* **RedactingTools scans MCP results ✓.** `redact.rs` runs the inner executor
  then scans the final string against every vault value (literal + JSON-escaped
  forms). `executor_for_chat` wraps *both* sides of the composite, so MCP output
  crosses the same scan. Proven by an adversarial test: an MCP `echo` returning a
  vault value came back `[REDACTED]`. Reverted.
* **Panic containment ✓.** `execute` wraps `dispatch` in `catch_unwind`; the
  worker itself `catch_unwind`s each job. A panicking rmcp client task cannot
  kill the process or poison the chat loop.
* **Live verification recorded.** Diary logs a real `lambo serve --session
  m10-live` child driven through `mcp.memory.*` over Cloud SQL + Vertex: derive →
  stats (1 concept, 1 embedded) → fresh-process recall relevance 1.07. Grant
  denial pinned offline.

## Mutation testing (each must be caught by a named test)

| # | Mutation | Location | Caught by | Result |
|---|----------|----------|-----------|--------|
| M1 | Disable expose filtering in `spawn_one` (admit every tool) | `mod.rs` `if !cfg.expose.contains(short) { continue; }` → always include | `specs_expose_only_the_allowlisted_tools` | **CAUGHT** (asserted uuid absent) |
| M2 | Remove the phase-3 transport respawn (fail immediately on Transport) | `execute_on_worker` replace respawn block with immediate `mcp_tool_failed` | `a_crashed_child_is_reconnected_on_the_next_call` | **CAUGHT** (second echo failed) |
| M3 | Bypass the gate: return the raw composite from `executor_for_chat` (drop `compose_chat_stack`) | `tools/mod.rs` | `mcp_tools_are_absent_without_a_grant_and_present_with_one` | **CAUGHT** (mcp.srv.echo surfaced with no grant) |

All three mutations reverted; tree clean at end.

## Gate table

| Gate | Command | Result |
|------|---------|--------|
| fmt | `cargo fmt --check` | PASS |
| clippy | `cargo clippy --locked -- -D warnings` | PASS |
| test | `cargo test --locked` | PASS (208 passed, 1 ignored) |

## Verdict

**APPROVE-with-minors.**

The core security posture — expose allowlist, vault-ref fail-closed at spawn,
single gate covering the composite, egress redaction over MCP results, bounded
one-respawn reconnect, panic containment — is correct and the load-bearing
behaviors are pinned by mutation-caught tests. The one substantive defect is
P2-1 (a wedged-but-alive child pins the single worker thread because `MCP_CALL_WAIT`
is never applied as an inner timeout to the `call_tool` RPC); it is real but
requires a deliberately non-answering server to trigger and does not break the
default fixtures. The two P3s (dot-containing server names unaddressed by the
`mcp.<server>.<tool>` split; `spawned` flag preventing later specs-level recovery)
are narrow and each has an obvious remediation. None blocks the milestone.