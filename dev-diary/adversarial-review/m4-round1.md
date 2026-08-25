# Adversarial review — Mooshik M4, round 1

**Reviewer**: independent, review-only. Wrote nothing under review except this file.
**Date**: 2026-08-25
**Scope**: commit `8dd9d9afb83580f1d479a28d041235bfb41374d3` on `m4-tool-surface`.
**Worktree**: `/tmp/mooshik-m4` @ `8dd9d9a`
**Verdict**: **REJECT** — 0 P1 / 3 P2 / 3 P3 (return for remediation; the contract pins hold, the gates do not)

## Method

Read the M4 milestone definition (`dev-diary/PLAN.md`), the implementation record
(`dev-diary/adversarial-review/m4-implementation.md`), and the full source:
`src/tools/{mod,schema,worker,scratch,tests}.rs`, `src/companion/{tools,chat}.rs`,
`src/cli.rs`, `src/text/en.toml`. Cross-checked the lifted schemas and the
`bad_param` / panic-containment discipline against the pinned Lambo rev
`f90a662` (`~/.cargo/git/checkouts/lambo-…/f90a662/src/mcp/server.rs`).
Mutation-tested every new pin: break the fix, run the named test
(`cargo test --locked --lib -- --exact <full::path>`, `running 1 test`),
restore. Transient edits fully reverted; the tree is clean except this file.
Gates run once at the end.

## Findings

| ID | Severity | Finding | Evidence |
| --- | --- | --- | --- |
| P2-M4-1 | **P2** | `run_script` reader joins are unbounded on the normal-exit path: a script that backgrounds a grandchild retaining the stdout/stderr pipe and exits 0 wedges the calling thread (the chat loop) until that grandchild closes the pipe. The sandbox's `setsid` + group-kill protects only the **timeout** path; on a clean exit `wait_child` returns immediately and `out_handle.join()` blocks without any wall-clock bound. `sleep 3 &` held the pipe 3s in the raw repro; `sleep 1000 &` held it indefinitely (`timeout 4` had to kill the reader). Mutation 7 demonstrated the same mechanism from the other side: kill-direct-child-only left the `sleep 60` grandchild holding the pipe for a full 60s. A model can trigger this with `nohup … &` in a scratch script, hanging the whole turn past every "hard timeout" guarantee. Fix direction: once the direct child is reaped, close the pipe ends after a short drain grace (kill the remaining group on exit, or bound the joins with a deadline and return captured output). | `src/tools/scratch.rs:175-193` (`join()` unbounded), `:196-219` (`wait_child` returns on direct-child exit), raw pipe repro, mutation 7 |
| P2-M4-2 | **P2** | HEAD fails the two standing gates that M2/M3 claimed green on. `cargo fmt --all -- --check` fails on `src/tools/worker.rs` (missing trailing blank line at EOF); `cargo clippy --all-targets --locked -- -D warnings` fails with 3 errors: `format!` in `eprintln!` args (`src/tools/mod.rs:351`), needless borrows (`src/tools/tests.rs:128`, `:138`). All pre-existing at `8dd9d9a`; mechanical fixes, zero behavior change. | gate runs, `git diff` empty (not reviewer residue) |
| P2-M4-3 | **P2** | The synchronous-path panic containment is unpinned. `execute` wraps `dispatch` in a second `catch_unwind` covering the tools that run on the caller thread (`lambo_stats`, `run_scratch_script`, `render_derive`, the `confirm` callback, door deserialization), but no test makes any of those panic. Mutation 3 (remove the outer `catch_unwind`) is **MISSED**: all 28 `tools` tests pass. A regression there would surface as a panicking chat thread (a dead process instead of an error string) with no failing test. The worker-level containment (`worker.rs:75`) is pinned; the caller-thread containment is not. Fix direction: a test that injects a panic through a synchronous tool (e.g. a panicking `confirm` closure, or a `stats`/derive path) and asserts an error string plus a still-usable executor. | mutation 3 **MISSED** |
| P3-M4-1 | **P3** | A timed-out-but-still-running recall/derive job permanently occupies the single worker thread (`worker_threads(1)`), so every later recall/derive times out until that job completes. Bounded wait protects the caller (documented tradeoff, `worker.rs:41-43`), but a genuinely hung backend degrades the whole memory surface, not just one call. Not a leak — the job does finish and free the worker in the bounded case. | `src/tools/worker.rs:64-76`, `:107-111` |
| P3-M4-2 | **P3** | Door cap unit divergence vs Lambo: Mooshik's `check_size` counts **characters** (matching the `schemars(length(max = 16_384))` semantics, tested at `schema.rs:298-304`); Lambo's door (`validate_size` / `MAX_CONTENT_BYTES`) counts **bytes**. Same cap value (16_384); for multibyte input Mooshik's door is looser than Lambo's door and tighter than nothing. Deliberate and tested; documented divergence, not a caps violation. | `schema.rs:35-41` vs `lambo/src/cli/caps.rs:214` |
| P3-M4-3 | **P3** | Out-of-scope + lift-fidelity verification only (not defects): no `delegate_to_coder` / `search_web` / `fetch_page` anywhere under `src/`; schemas verified verbatim against pinned Lambo `f90a662` (`RecallParams`, `WireConcept`, `WireParentOf`, `DeriveParams`, `WireResource`, `RecordActionParams`, `StatsParams` — same `deny_unknown_fields`, same caps), with the M4 door adding a 64-concept derive cap Lambo lacks (superset, safe). Egress: Lambo error detail goes to `eprintln` (local), the model gets a generic string — no DSN/secret path found; SecretToken / `init_tracing` are out of M4 scope (M6 / M10). `lambo_stats.receipt` honestly reported as `never-issued`. | source + Lambo rev diff |

## Mutation table

Every listed run executed exactly the named test (`running 1 test`); all
transient edits reverted, tree clean afterwards.

| # | Pin | Mutation | Result |
| --- | --- | --- | --- |
| 1 | Async seam bounded wait | `rx.recv_timeout(timeout)` → unbounded `rx.recv()` | **CAUGHT** `tools::worker::tests::timed_out_job_does_not_kill_the_worker` |
| 2 | Worker panic containment (async path) | drop `catch_unwind` around `job(&rt)` in worker loop | **CAUGHT** `tools::worker::tests::panicking_job_is_contained_and_the_worker_survives` |
| 3 | `execute` sync-path panic containment | drop outer `catch_unwind` in `ToolExecutor::execute` | **MISSED** — all 28 `tools` tests pass (P2-M4-3) |
| 4 | Scratch permission prompt fail-closed | remove the `(self.scratch.confirm)` gate in `run_scratch` | **CAUGHT** `tools::tests::scratch_is_denied_when_confirmation_is_refused` |
| 5 | Schema caps survive | `length(max = 65_536)` on `RecallParams.query` | **CAUGHT** `tools::schema::tests::recall_params_schema_is_an_object_with_required_fields` |
| 6 | Schema `deny_unknown_fields` | remove attribute from `RecallParams` | **CAUGHT** `tools::tests::unknown_field_is_refused_as_a_tool_error` |
| 7 | Scratch hard timeout kills whole process group | `kill(-pgid)` → direct-child `kill()` only | **CAUGHT** `tools::scratch::tests::hard_timeout_kills_the_child` (60s hang, elapsed assertion) |
| 8 | Scratch output cap enforced | `read_capped`: `room = usize::MAX` | **CAUGHT** `tools::scratch::tests::output_is_capped` |
| 9 | M3 pin `chat_dispatch_does_not_open_memory` | `memory::provision` reference in `fn chat` body | **CAUGHT** `cli::tests::chat_dispatch_does_not_open_memory` |
| 10 | M3 pin `run_chat_does_not_open_memory` | `memory::` reference in `chat.rs` production | **CAUGHT** `companion::chat::tests::run_chat_does_not_open_memory` |

**Mutation score**: 9/10 required pins fail under mutation; the single MISSED
is the synchronous-path panic containment (P2-M4-3).

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **FAIL** — `src/tools/worker.rs` EOF (missing trailing blank line), pre-existing at HEAD |
| `cargo clippy --all-targets --locked -- -D warnings` | **FAIL** — `mod.rs:351` (`format!` in `eprintln!`), `tests.rs:128`, `tests.rs:138` (needless borrows), pre-existing at HEAD |
| `cargo test --locked` | **PASS** — 125 passed, 0 failed, 1 ignored (`live_postgres_and_gemini_round_trip`, untouched) |
| File-size cap | **PASS** — largest `src/secure_path/mod.rs` 792; `tools/` files ≤ 467 |
| Default tests touch network/model | **PASS** — fixture memory + fast `MissingDsn` (no network); live GCP round trip ignored |
| Out-of-scope tools absent | **PASS** — no `delegate_to_coder` / `search_web` / `fetch_page` under `src/` |
| M3 pins hold literally | **PASS** — source scans green; mutations 9/10 CAUGHT |

## Conclusion

**REJECT** — return for a remediation round (the M3 pattern: P2s at round 1 → addressed → round-2 APPROVE).

The M4 contract pins are largely held: the async seam survives panics and
timeouts without killing the worker (mutations 1–2 CAUGHT), the scratch
permission prompt fails closed (4), schema caps and `deny_unknown_fields`
survive (5–6), the process-group kill and output cap hold (7–8), the two M3
memory pins hold literally and under mutation (9–10), default tests are
net-free, and the four in-scope tools exist behind the synchronous seam with
no out-of-scope tools. Three things must be fixed before this lands as green:
the unbounded reader-join hang on the scratch normal-exit path (P2-M4-1), the
red fmt/clippy gates (P2-M4-2), and a pin for the synchronous-path panic
containment (P2-M4-3). Minors (P3-M4-1…3) are documentary; no P1.