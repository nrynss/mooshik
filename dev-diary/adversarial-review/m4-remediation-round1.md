# Adversarial review remediation — Mooshik M4, round 1

**Date**: 2026-08-25
**Target**: worktree `/tmp/mooshik-m4` on `m4-tool-surface` (HEAD `8dd9d9a`), after
review round 1 (`m4-round1.md`, verdict REJECT — 0 P1 / 3 P2 / 3 P3).
**Scope**: the three P2s only. The P3s are documentary and left as-is; the design
decisions recorded in `m4-implementation.md` are unchanged.

Gates re-run at the end on this tree; all green.

## P2-M4-1 — unbounded reader join on the clean-exit path

**Finding**: `run_script` joined the stdout/stderr reader threads with an
unbounded `.join()` after `wait_child` returned. `setsid` + group-kill protected
only the timeout path; on a clean exit a backgrounded grandchild retaining the
pipes wedged the calling (chat) thread until that grandchild closed them
(`sleep 3 &` → 3s; `sleep 1000 &` → indefinite).

**Change** (`src/tools/scratch.rs`): the two `.join()` calls are replaced by a
single bounded reader-join, `join_readers`, applied to both handles with one
shared deadline. Same wall-clock discipline as the timeout path:

- give the readers one `timeout` budget (`Instant::now() + timeout`);
- if a reader is still blocked at expiry, kill the child's whole process group
  (the child ran under `setsid`), so a pipe-holding grandchild drains the
  readers — the same `kill(-pgid)` used on the timeout path;
- give the readers one more budget; if one is *still* blocked after the group
  kill (a descendant that escaped the group, e.g. a double-forked `setsid`),
  return the io error and drop the handles, which detaches the reader threads
  rather than wedging the caller.

The finish-detection uses `JoinHandle::is_finished` + `take()`; no channel, no
extra allocation. This bounds the reader join on **both** paths (timeout-kill
and clean exit), not just the timeout path.

**Coverage**: added `tools::scratch::tests::clean_exit_with_background_grandchild_bounds_the_reader_join`
— a `sleep 100 &` script that exits 0 but leaves a grandchild holding both
pipes; asserts `exit_code == Some(0)`, `!timed_out`, and elapsed `< 10s`. The
existing `hard_timeout_kills_the_child` still passes unchanged.

**Mutation result (new pin)**: temporarily reverted the call site to the two
unbounded `.join()` calls; the new test ran past `timeout 12` and was killed
(exit 124) — **CAUGHT**. Restored; the test completes in well under 2s. Any
stray `sleep` orphans from the mutation were killed; none remain.

## P2-M4-2 — fmt / clippy gates red

**Finding**: `cargo fmt --all -- --check` failed on `src/tools/worker.rs` (missing
trailing blank line at EOF); `cargo clippy --all-targets --locked -- -D warnings`
failed with `format!` in `eprintln!` args (`src/tools/mod.rs:351`) and needless
borrows (`src/tools/tests.rs:128,138`).

**Change**: mechanical only, zero behavior change.

- `src/tools/mod.rs` `lambo_err`: `eprintln!("{what}: memory error: {}", format!("{error}"))`
  → `eprintln!("{what}: memory error: {error}")`.
- `src/tools/tests.rs`: dropped the needless `&` before
  `crate::text::get("tools.bad_param")` (line 128) and
  `crate::text::get("tools.range_error")` (line 138).
- `cargo fmt --all` applied (normalizes the `worker.rs` EOF blank line and
  reflows several pre-existing non-conforming lines across `tools/`, `lib.rs`,
  `companion/`).

## P2-M4-3 — pin the synchronous-path panic containment

**Finding**: `execute` wraps `dispatch` in a second `catch_unwind` covering the
caller-thread tools (stats, scratch, the `confirm` callback, door
deserialization, derive render), but no test made any panic — mutation 3 (drop
the outer `catch_unwind`) was MISSED by all 28 tools tests. A regression would
surface only as a dead chat process.

**Change** (`src/tools/tests.rs`): added
`tools::tests::a_panicking_sync_tool_is_contained_as_an_error_string` — a
panicking `confirm` closure (`|_| panic!("confirm exploded")`) routed through a
confirmed scratch call on the caller thread; asserts `execute` returns the
contained generic error string (`tools.internal_error`), not a panic.

**Mutation result (the new pin)**: temporarily removed the outer `catch_unwind`
in `ToolExecutor::execute`; the named test FAILED with the raw panic
(`confirm exploded`, thread `tools::tests::a_panicking_sync_tool_is_contained_as_an_error_string`)
propagating out of `execute` — **CAUGHT**. Restored the containment; the test
passes. The worker-level containment (`worker.rs:75`) was already pinned and
remains green.

## Gate summary (this tree, after all edits)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** |
| `cargo test --locked` | **PASS** — 127 passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`, untouched) |

Test count: 125 → 127 (the two new pins). P3s unchanged and still documentary;
M4 design decisions from `m4-implementation.md` hold.