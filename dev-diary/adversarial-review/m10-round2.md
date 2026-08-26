# M10 round 2 — adversarial re-verification of the round-1 remediation

Reviewed at `f50765f` (branch `m10-mcp-host`), worktree `/tmp/mooshik-m10`.
Scope: the round-1 closed items (P2-M10-1 per-call bound, P3-M10-1 dotted-key
rejection, P3-M10-2 false-on-failure spawn flag) plus the round-1 minor set
(empty-expose inertness ordering, `MCP_CALL_WAIT` vs `MCP_SPAWN_WAIT` budget,
`internal_error` on TimedOut). Every claim below is executed or directly
traced from source, not read off; all transient edits fully reverted.

## Findings

### P1

None.

### P2

None. The round-1 P2 (a wedged-but-alive child pins the single worker thread)
is closed and now pinned by a test.

Both `call_tool` sites route through `bounded_call` with the same
`self.call_wait`:

- **Phase 2** (`mod.rs:480`): `let result = bounded_call(&live, tool, arguments, call_wait).await;`
- **Phase-3 retry** (`mod.rs:502`): `let result = bounded_call(&revived, tool, arguments, call_wait).await;`

`bounded_call` (`mod.rs:443-452`) wraps `live.call_tool(...)` in
`tokio::time::timeout(wait, ...)`, mapping a timeout to
`Err(CallError::Transport)` — which on phase 3 falls into the respawn/retry
branch (bounded) or, on the retry's own transport failure, returns the
contained `mcp_tool_failed`. So a non-answering child frees the worker after
`call_wait`; the worker thread is never pinned.

The outer budget is correctly widened: `dispatch` (the only `worker.run` caller
for an MCP tool execute) passes `call_wait + MCP_SPAWN_WAIT` (`mod.rs:278`),
because an execute may perform one phase-1 spawn (≤ `MCP_SPAWN_WAIT`) plus one
bounded call (≤ `call_wait`) plus, after a transport failure, one more spawn
and one more bounded call. The caller-side `recv_timeout` therefore no longer
fires first as the de-facto inner bound. `attempt_spawn`'s budget is
`MCP_SPAWN_WAIT * n` (one spawn pass; no calls in that path), correct and
unchanged. Defaults: `MCP_CALL_WAIT = 60s`, `MCP_SPAWN_WAIT = 30s`, so a
production hung call frees the worker at 60s and a transport-respawn sequence
bounded at 60+30+30+60 = 180s worst case — all finite.

**Mutation M2-1 (bypass the phase-2 bound):** replaced line 480 with
`live.call_tool(tool, arguments).await` (removing only the `timeout` wrapper).
The pin `a_hung_but_alive_child_does_not_pin_the_worker` (exposes `hang`,
`call_wait = 300ms`) **HUNG instead of passing** — my run was bounded
externally with `timeout 180` and exited `124`, with the test still marked
"running for over 60 seconds" (its worker parked on the never-answering child,
exactly the round-1 failure mode). Reverted; the pin passes at
`call_wait = 300ms` in ~0.97s. **CAUGHT.**

Fixture verification: the `hang` tool is genuinely wired —
`fixture_server.py` lists `hang` in `TOOLS` (never-answer description) and the
`tools/call` handler `time.sleep(300)`s for it (`fixture_server.py:66-70,133-134`),
so the call is issued to a child that parses the request and answers nothing.
The pin exposes both `echo` and `hang` and drives warm `echo` → hung `hang` →
recovered `echo`, proving the worker survives.

### P3

None new. Both round-1 P3s are closed and pinned.

**P3-M10-1 (dotted server key):** `validate_mcp` (`config/mod.rs:420-444`)
rejects any `mcp_servers` key containing `.` with `ConfigError::InvalidMcp` as
the **first** check per entry, before command/env-field validation. The check
applies to **every** entry regardless of `expose` — a dotted key with `expose =
[]` still fails the load (the `name.contains('.')` branch precedes the expose
semantics; empty expose is only ever *not-an-error* when the key is otherwise
valid). So there is no expose-order gap: a dotted inert server is rejected, not
silently dropped. The pin `config::tests::a_dotted_server_key_fails_closed`
(`[mcp_servers."github.app"]`) asserts `from_toml_and_env` →
`Err(ConfigError::InvalidMcp)`; run green. The mcp_host side is unreachable for
dotted keys because `McpTools::from_config` is only ever handed a `Config` that
already passed `validate_mcp` (called in `overlay.rs:41` on every load path),
and `parse_mcp_name` still guards against a dotted name defensively.

**P3-M10-2 (spawned flag on failure):** `ensure_spawned` (`mod.rs:197-209`)
sets `*spawned = true` **only if** `attempt_spawn()` reports that every
non-inert server has a live slot; on any failure it leaves `spawned = false`,
so a later `specs()`/`execute` re-enters and retries the spawn pass.
`attempt_spawn` (`mod.rs:213-228`) is the honest check: it runs `spawn_all` on
the worker, `refresh_specs()` from whatever slots hold, then returns
`ok && lives.iter().all(|slot| slot.lock().is_some())`. A vault-missing secret
fails the spawn → `spawned` stays false → next `specs()` retries.

The existing pin `a_later_call_after_a_dead_initial_spawn_recovers` uses
**crash → echo**, which exercises the per-execute phase-1 respawn, **not** the
specs-level retry — the assignment's concern was right. I probed the genuine
failed-*initial*-spawn → specs-retry path behaviorally: a server whose `env`
names a secret absent from the vault at the first `specs()` (spawn fails, no
tool surfaced) then has the secret `set` into the same `SharedVault`; a second
`specs()` **must** surface the tool. That probe **passed** on the remediated
code, and under the pre-remediation mutation (make `ensure_spawned` set
`spawned = true` unconditionally, always retrying `attempt_spawn` in the original
code) it **failed** with `later specs() must recover: []` (the pre-fix
dead-slot caching). Probe reverted, not committed. So the fix is behavioral,
not just traced, and the code path that `a_later_call_after_a_dead_initial_spawn_recovers`
does not cover is genuinely fixed.

## What held up under attack

* **Both call sites bound; no call_tool escapes.** Only two `call_tool` awaits
  exist (`mod.rs:480,502`), both under `bounded_call`. No third site.
* **Phase-3 transport-recovery still bounded.** After a timed-out/transport
  `call_tool`, the respawn (`mod.rs:497`) is `timeout(MCP_SPAWN_WAIT, …)` and
  the retry (`mod.rs:502`) is `bounded_call(…, call_wait)`; the slot-lock is
  still never held across an await (each phase locks/takes/sets). Both produce
  contained strings; `isError` (`ToolError`) still returns the server text and
  restores the live session without respawn, unchanged.
* **Timeout error mapping (genial/contained) preserved.** `bounded_call` maps a
  per-call timeout to `Err(CallError::Transport)` — it is a *transport-level*
  failure from the caller's perspective, exactly the branch that respawns once
  and otherwise yields the contained `mcp_tool_failed`. The outer
  `ToolRuntime::run` `TimedOut` → `tools.tool_timeout` → `internal_error` path
  (`mod.rs:289-293`) is unchanged and unreachable unless the *entire* bounded
  budget is consumed. No client-visible detail leaks on timeout.
* **`with_call_wait` is `#[cfg(test)]`-only.** Defined under `#[cfg(test)]`
  (`mod.rs:189-193`); the sole call is in `tests.rs:129`. It cannot leak into
  production — the `call_wait` field initializes to `MCP_CALL_WAIT` in
  `from_config` and nothing in a non-test build can overwrite it.
* **Fixture exposes the hungry tool only via the allowlist.** The pin uses
  `&["echo", "hang"]`, so `hang` is only reachable when explicitly exposed —
  consistent with the fail-closed expose semantics.
* **Config caps untouched.** `MAX_CONFIG_BYTES = 64 * 1024` still applies at
  both `Config::load` and `load_at`; `open_sensitive` behavior unchanged.
* **en.toml / PLAN / filecaps untouched by the remediation.** The `f50765f`
  diff touches only `dev-diary/adversarial-review/m10-round1.md`,
  `src/config/mod.rs`, `src/mcp_host/mod.rs`, `src/mcp_host/tests.rs`,
  `src/mcp_host/tests/fixture_server.py` — no text changes, no plan drift, no
  new caps keys. `src/text/en.toml` has no `filecaps` section and all the
  `tools.mcp_*` keys the code consumes are present.
* **Cross-server / isError / gate / redaction behavior** (round-1 verified)
  unchanged by this commit; the diff is additive and locally bounded.

## Mutation testing (each must be caught by a named test)

| # | Mutation | Location | Caught by | Result |
|---|----------|----------|-----------|--------|
| M2-1 | Bypass the per-call bound on phase 2 (`live.call_tool(...).await` directly, dropping `bounded_call`) | `mcp_host/mod.rs` phase-2 site (line 480) | `a_hung_but_alive_child_does_not_pin_the_worker` | **CAUGHT** — pin HANGS (my run externally bounded at 180s → exit 124, "running for over 60 seconds"); passes in ~0.97s when restored |
| M3-2 | Make `ensure_spawned` set `spawned = true` unconditionally (always short-circuit retries after one failed pass) | `mcp_host/mod.rs` `ensure_spawned` | *probe* `transient_specs_retry_is_probed_proportionally` (TEMP, not committed) | **CAUGHT** — probe FAILS (`later specs() must recover: []`); passes on remediated code |
| — | (verification-only) dotted-key rejection | `config` load | `config::tests::a_dotted_server_key_fails_closed` | PASS (no mutation needed; pin green) |

Both mutations fully reverted; tree clean except the review doc.

## Gate table

| Gate | Command | Result |
|------|---------|--------|
| fmt | `cargo fmt --check` | PASS |
| clippy | `cargo clippy --locked -- -D warnings` | PASS |
| test | `cargo test --locked` | PASS (211 passed, 0 failed, 1 ignored) |

(The 1 ignored test is the suite's pre-existing `report_pin` skip, unrelated to
M10. One transient `BrokenPipeError` in the fixture stderr is the expected
by-product of the crash-child test closing the pipe; the suite exits 0.)

## Verdict

**APPROVE.** Zero P1/P2/P3 residue.

- The round-1 P2 is genuinely closed: both `call_tool` sites run under
  `bounded_call(self.call_wait)`, the outer budget is `call_wait + MCP_SPAWN_WAIT`
  so the caller-side deadline no longer replaces the inner bound, and a
  mutation dropping the phase-2 timeout makes the pin hang (CAUGHT). The `hang`
  fixture is truly non-answering and both `echo`/`hang` are exercised.
- The round-1 P3s are closed: dotted keys fail config load *for every entry*
  (expose-order included), and `ensure_spawned`'s false-on-failure provably
  retries a failed initial spawn on a later `specs()` (behavioral probe,
  mutation-caught, reverted).
- The `call_wait` change did not disturb the timeout/`internal_error` mapping
  (`bounded_call`→`Transport`→contained), `with_call_wait` is `#[cfg(test)]`-only,
  config file caps and en.toml/PLAN are untouched, and there are no `PLpdates`.