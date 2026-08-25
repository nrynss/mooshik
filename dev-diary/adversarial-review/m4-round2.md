# Adversarial review — Mooshik M4, round 2

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-25
**Scope**: round-1 remediation on `m4-tool-surface` @ `5e3d113`, against the three
P2 findings in `m4-round1.md` and the claimed fixes in
`m4-remediation-round1.md`.
**Worktree**: `/tmp/mooshik-m4` @ `5e3d113` (tree clean at completion; only this
file untracked).
**Verdict**: **APPROVE** — 0 P1 / 0 P2 / 1 P3 (documentary, pre-existing
tradeoff; no new residue introduced by the remediation).

## Method

Independently traced the three P2 fixes in source — `src/tools/scratch.rs`
(`join_readers` bounded join with shared deadline + group kill + detach-give-up),
`src/tools/mod.rs` (`lambo_err` eprintln + `execute` catch_unwind), `src/tools/
tests.rs` (the two new pins), `src/tools/worker.rs` — then mutation-tested each
closure against its named pin (break the fix → run the exact test, restore).
Separately probed the two residue hypotheses the remediation could introduce
(detach-give-up path; stdout+stderr split drain). All transient edits and probes
fully reverted; `git status` clean. Gates run once at the end.

## Findings

| ID | Severity | Finding | Evidence |
| --- | --- | --- | --- |
| P3-M4-4 | **P3** | The detach-give-up branch of `join_readers` (a pipe-holder that escaped the sandbox process group — e.g. a double-forked `setsid`) is unpinned by any committed test, and its two reader threads are dropped (detached) and stay alive until the escaped descendant closes the pipes. The caller *is* bounded (probe returned `Err` in 624 ms, not wedged), so this is not a round-1-style wedge; the lingering is bounded by the escaped process's lifetime, not by the caller's need. Deliberate, documented give-up tradeoff; noted for completeness, not a new P2. | temporary probe `setsid sleep 100 &` → 300 ms budgets → `Err` ≈624 ms; `join_readers:272-291`; doc `scratch.rs:230-233` |

**No new P1/P2 residue.** Explicit round-1 questions all resolve in the
remediation's favor:

- **Does the reader deadline handle both stdout+stderr correctly?** Yes — the
  loop tracks `out` and `err` handles independently every iteration and the
  group kill drains whichever pipe is still held. Temporary probe with one
  stdout-holding grandchild (`sleep 100 2>/dev/null &`) and one stderr-holding
  grandchild (`sleep 100 1>/dev/null &`) returned clean `Ok` (exit 0, `!timed_out`)
  in 342 ms (group kill at the 300 ms deadline drained both pipes).
- **Can a grandchild of a NON-backgrounded script still wedge the caller?** No
  caller wedge. A descendant still in the sandbox group is killed by the group
  kill; one that escaped the group (setsid) is covered by the bounded give-up
  (`Err`, caller returns in ~2×timeout). The only persistent cost is the
  detached reader threads (P3-M4-4).
- **Does the detach-give-up path leak a thread long-term?** Bounded caller; the
  two detached reader threads persist until the escaped pipe-holder exits. Not a
  caller-bound leak; documented tradeoff (P3-M4-4).
- **Did the clippy fix change any error path?** No. `lambo_err`:
  `eprintln!("{what}: memory error: {}", format!("{error}"))` →
  `eprintln!("{what}: memory error: {error}")` — Display output byte-identical,
  `LamboError` detail still goes to stderr, model still gets the generic string.
  The `tests.rs` needless-borrow drops and `worker.rs` EOF blank line are
  formatting-only. Zero behavior change.

## Mutation table

Every listed run executed exactly the named test (`running 1 test`); all
transient edits reverted, tree clean afterwards.

| # | Pin | Mutation | Result |
| --- | --- | --- | --- |
| 1 | P2-M4-1 bounded reader join | restore call site to two unbounded `.join()` | **CAUGHT** `tools::scratch::tests::clean_exit_with_background_grandchild_bounds_the_reader_join` — hung past `timeout 20`, exit 124 |
| 2 | P2-M4-3 sync-path panic containment | drop outer `catch_unwind` in `ToolExecutor::execute` | **CAUGHT** `tools::tests::a_panicking_sync_tool_is_contained_as_an_error_string` — test FAILED, raw `confirm exploded` propagated out of `execute` |

**Mutation score**: 2/2 required round-1 pins CAUGHT under independent mutation
(round 1 was 9/10 with P2-M4-1 and P2-M4-3 missing; both now pinned). P2-M4-2 is
a gate, verified directly.

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **PASS** (fixes `worker.rs` EOF + reflow; no output) |
| `cargo clippy --all-targets --locked -- -D warnings` | **PASS** (0 errors; fixes `mod.rs` eprintln, `tests.rs` needless borrows) |
| `cargo test --locked` | **PASS** — 127 passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`, untouched) |
| File-size cap (≤1000 lines) | **PASS** — largest `src/secure_path/mod.rs` 792; `tools/` ≤ 517 |
| Default tests net/model-free | **PASS** — fixture memory; live GCP round trip ignored |
| Round-1 mutations still hold | **PASS** — worker/mutation pins 1–2 of round-1 unchanged (source traced) |

## Conclusion

**APPROVE.** All three round-1 P2s close under independent trace and mutation:
P2-M4-1 (bounded reader join) is CAUGHT by `clean_exit_with_background_grandchild_bounds_the_reader_join`
(mutation hangs, exit 124), P2-M4-3 (sync-path containment) is CAUGHT by
`a_panicking_sync_tool_is_contained_as_an_error_string` (mutation FAILS with the
raw panic), and P2-M4-2 (fmt/clippy) is green on the live gates. Mutation score
9/10 → 2/2 on the round-2 targets; gates 125→127 tests, all green. The only
remaining item is P3-M4-4 (documentary): the detach-give-up branch is unpinned
and its detached readers live until an escaped descendant closes its pipes —
bounded for the caller, inherent to the design, not a wedge. No new P1/P2. This
lands green.