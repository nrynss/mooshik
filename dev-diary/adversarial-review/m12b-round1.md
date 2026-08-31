# M12b round 1 — adversarial review of the tick

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
(10 modified files + 1 new test file, no commits — the full expected set, verified
identical before and after every mutation). Scope: the `ViewData` seam in
`src/memory/view.rs` (stats → one guard → one copy → build against the copies,
`of_graph` private), the tick (`src/tui/mod.rs`, `src/tui/app.rs`,
`src/cli/tui_cmd.rs`), the measurement harness (`src/memory/view_tick_tests.rs`),
the M12a regression pins, and the no-behaviour-change check. All transient
mutations reverted and `sha256sum`-verified identical to the pre-mutation state
(`937cd497…`); `git status --porcelain` shows exactly the same 10 modified + 1
untracked as before I started, now with this record beside them. Nothing committed.
Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so every `cargo` invocation ran
under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 1 × P2, 1 × P3.

The milestone itself is done and holds up: the tick rebuilds the view model
through a clean seam, a write from elsewhere genuinely appears without a
keystroke (executed end to end against a real sqlite `Memory`), the rebuild cost
fits the 250 ms budget with the claimed margins (measured by me, not taken from
the record), the R1-3 deferral's structural half is genuinely closed (`of_graph`
private, takes `&ViewData`, `of_memory` the only route from a `&Memory` to a
`Workspace`), the demo path never rebuilds, and M12a's pins all still hold.
Both documented mutations fail their pins with the named reasons, verbatim.

What is wrong is the edge of the **starvation pin**. `the_build_runs_against_the_copy_and_not_the_guard`
checks only textual order — guard, then copy, then build — and the flat
three-statement form (`let guard = …; let graph = ViewData::from_graph(&guard);
of_graph(&stats, &graph, now)`) satisfies that order while keeping the guard
alive for the **entire build**: the exact writer-starvation fault the copy
exists to prevent, re-introduced by a plausible simplification, passes the pin
three runs out of three. Its own doc's justification — "that is the order the
code must be written in for the guard to be gone by the time the build runs" —
is false for that form. By the round-2 precedent (R2-2: a pin passing with the
hazard present is a P2), this is a P2, and per this round's instruction there
are no deferrals.

## What held up under attack

* **The ViewData seam is genuinely short.** `of_memory` reads `stats()` (which
  takes the graph lock itself and releases it), takes one guard for exactly the
  copy, and the build reads only `ViewData`. `of_graph` is private, takes
  `&ViewData` and nothing else — a guard can never reach the build without
  changing a signature. Grep over `src/` confirms the only production callers
  of `of_memory` are `tui_cmd::live`'s two calls (first model + tick closure);
  `of_graph` is called only inside `view.rs` and its own test modules.
* **Mutation (a) bites.** Stats moved after the copy (guard before the figures):
  `the_figures_are_read_before_the_graph_guard` fails in 0.00 s with the deadlock
  reason, verbatim. Reverted, hash-verified, pin green.
* **Mutation (b) bites.** Guard binding taken, then
  `of_graph(&stats, &ViewData::from_graph(&guard), now)` — the build under the
  held guard: `the_build_runs_against_the_copy_and_not_the_guard` fails in
  0.00 s with the starvation reason, verbatim. Reverted, hash-verified, pin green.
* **Mutation (c) survives — the P2.** The flat three-statement form above (guard
  binding alive across the whole build) passes the pin. Rust drops a `Drop`
  value at the end of its scope, not at its last use — `guard` is a
  `parking_lot::RwLockReadGuard` never moved, so it is held through
  `of_graph`'s ~25 ms debug run on every tick. The pin's doc overclaims what the
  source-order check can see.
* **The tick works, and keys are not dropped.** On a `poll(TICK)` timeout the
  loop calls `on_tick(app, refresh)` before the next draw, so the fresh model is
  what the next frame draws. A key arriving at the same instant as a tick simply
  defers the rebuild one iteration — the key is processed, and the next quiet
  tick catches the graph up, exactly as the doc says. No path loses either the
  key or the rebuild.
* **The demo path truly never rebuilds.** `tui_cmd::tui` hands `draw(…, None)`
  for `--demo` (no memory opened), `run`/`event_loop` carry `None` through, and
  `on_tick(app, None)` is a no-op. Pinned by
  `a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`, and smoke-run
  by me: `mooshik tui --demo` renders the full workspace on the alternate
  screen and exits 0 on `Esc`.
* **`App::refresh` keeps the user's place.** The conversation (draft) and
  `week.selected` are carried across the swap; view/focus/cursors/columns/rows
  are `App` fields and untouched. A smaller model is safe: `window_start`
  clamps `cursor.min(list.len().saturating_sub(1))` and `selected_in` guards
  `selected >= days.len()`, and `a_refresh_onto_a_smaller_model_does_not_break_the_next_draw`
  draws it.
* **The live path still closes the session on the way out.** `tui_cmd::live`
  opens `memory` (lease), draws through the loop, then `runtime.block_on(memory.close())`
  and reports the two outcomes in the M12a order. The refresh closure borrows
  `&memory` and is released before `close`; unchanged behaviour.
* **The measurement is honest, and I re-ran it.** My debug numbers: 1k/4k
  unembedded whole-rebuild means **7.1 ms / 27.5 ms** (report: 8.3 / 28.7);
  embedded 512 ms / 1.78 s (report: 483 ms / 1.86 s). Release: unembedded
  **0.82 ms / 3.2 ms** (report: 0.88 / 2.9); embedded 1k **28.1–28.5 ms**
  (report: 29), embedded 4k **109.7 ms** on two consecutive runs (report: 108;
  one cold run read 135). The budget holds with ~9× margin in debug at the
  M12a-comparable 4k shape and ~2.3× in release embedded — the claimed margins
  reproduce. The harness shapes match the M12a records exactly: I verified at
  the pinned lambo (`4c6fc93`) that `insert_concept` replaces a node by id and
  appends a fresh `Derives` edge per turn, so `turns` turns × 1 concept each
  with a `Temporal` chain gives 400/1 500 distinct concept nodes and
  1 999 / 7 999 edges — the assert in the harness checks `2·turns − 1` — and
  `draws_everywhere` proves the timed workspace draws at every size.
* **The rebuild-sees-a-write pin is real.** `a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`
  runs a real sqlite `Memory`, calls the live closure, `derive`s a concept, and
  the second closure answer shows it in the trickle; the fresh model is proven
  drawable. This is the milestone's central behaviour, executed, not asserted.
* **M12a regression pins all hold.** `the_local_database_is_created_and_repaired_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session` and
  `the_scratch_sandbox_and_script_stay_private` all pass on the M12b tree, and
  the `view_clock_tests`/`view_tests` diffs are mechanical re-plumbing
  (`of_graph(…, &graph, …)` → `of_graph(…, &ViewData::from_graph(&graph), …)`),
  nothing weakened.
* **No behaviour change beyond M12b.** Every hunk of every file read against
  the current tree: the diff is exactly the expected set (PLAN.md bullet,
  view.rs restructure, tui tick seam, App::refresh, tui_cmd wiring + source-pin
  delimiter, test re-plumbing, the new tick test file). `PLAN.md`'s M12b bullet
  now carries the R1-3 guard-duration item and is accurate as far as the pins
  actually reach — which is the one reservation below.
* **The demo smoke and lambo pin.** `mooshik tui --demo` renders and exits 0;
  lambo still pinned at `4c6fc93` in `Cargo.lock`.

## Findings

### P2

**M12b-R1-1 — `the_build_runs_against_the_copy_and_not_the_guard` passes when the
guard is held across the build.**

The pin proves only that the text reads guard → copy → build; it cannot see the
guard's scope. The flat form

```rust
let stats = memory.stats();
let guard = memory.graph().read();
let graph = ViewData::from_graph(&guard);
of_graph(&stats, &graph, now)
```

has exactly that order and compiles, while `guard` (a `RwLockReadGuard`, dropped
at the end of its scope, never at its last use) stays held for the whole
`of_graph` pass — the writer starvation at a 250 ms tick that the copy exists to
prevent, re-introduced by the natural simplification of the block-scoped shape.
Executed: this mutation passes the pin, three runs out of three. The pin's own
doc overclaims what the check sees: "guard, then copy, then build — because that
is the order the code must be written in for the guard to be gone by the time
the build runs" is false for this form, which is why the mutation is green. The
structural half of the R1-3 close (`of_graph` private, takes `&ViewData`) holds
against this — the hazard is purely the guard's lifetime — which is exactly why
the pin, the only thing left standing between a future simplification and the
fault, must see it.

*Remediation.* Assert the copy's block closes before the build: require a `}`
between the copy and the build call — e.g.
`body[copy..build].find('}').expect("the copy's block closes before the build")`
added to the existing `guard < copy && copy < build` assert. The correct shape
(`let graph = { … };` then the call) contains it; the flat form and the
inline-in-call form both lack it (the latter already fails the existing assert).

### P3

**M12b-R1-2 — the doc's "verifies the records' 6.5 ms / 18.2 ms" does not hold at
the 4k shape.**

`of_memory`'s doc says the measurement "verifies the records' 6.5 ms / 18.2 ms
for the build itself, plus the copy". The 1k leg reproduces (my build-only mean
6.6 ms vs the record's 6.5). The 4k leg does not: my build-only mean is
24.0 ms against the record's 18.2 — and the code is no longer the same build,
since M12b folds the `Derives` in-neighbour collection into the pass. The doc's
own headline numbers are accurate (my whole-rebuild means 7.1 / 27.5 ms match
the doc's ~8 / ~29), so this is the internal "verifies the records" sentence
claiming a comparison that doesn't reproduce, not a budget problem.

*Remediation.* Reword to compare like with like: "the M12a records measured
6.5 ms / 18.2 ms for the pre-copy build; this milestone's build, with the
`Derives` map folded into the same pass, measures ~8 ms / ~29 ms whole at the
same shapes, of which the copy is ~0.6 ms / ~3.5 ms."

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`937cd4972f8cea9d8aa74564e9db1b49f9b4e3bec19ce0c6053a8fa2a44c72cb`).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) stats moved after the copy — guard taken before the figures | `the_figures_are_read_before_the_graph_guard` | **caught** — "of_memory takes the graph guard before reading the figures, which deadlocks under a queued writer", verbatim vs the record |
| (b) guard binding taken, build fed `ViewData::from_graph(&guard)` inside the call | `the_build_runs_against_the_copy_and_not_the_guard` | **caught** — "the current order holds the guard across the build, which starves a writer at a 250 ms tick", verbatim vs the record |
| (c) flat three-statement form — guard binding alive across the whole build | `the_build_runs_against_the_copy_and_not_the_guard` | **SURVIVES** — 3 runs, 3 passes (P2, M12b-R1-1) |

Both shipped-code mutations reproduce the record's quoted outputs byte-for-byte;
each reverted run confirms the pin green on the shipped tree.

## Gates

Run by me at the end, in a clean env (all three DSN variables unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (both
  pre-existing and legitimately marked: `live_postgres_and_gemini_round_trip`
  live-Cloud, `eyeball` print-only) **+ 1 integration passed**
  (`tests/report_pin.rs`) **+ 0 doc**, exit 0. Matches the implementation's
  numbers exactly. Full run ~5 min wall; the lib phase 14.45 s of which the
  tick measurement test is ~13.5 s (the embedded legs, which are measured but
  unasserted, dominate — see Notes).
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `view.rs` 974 (the implementation's own number; was
  875), `tui/mod.rs` 807, `view_tests.rs` 871, `cli/tests.rs` 811,
  `tui_cmd.rs` 119, `view_tick_tests.rs` 169 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`).

## What was executed vs. only read

**Executed.** All three mutations (a, b, c), each reverted and hash-verified,
with the pin runs before and after. The two headline pins
(`a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
`a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`), both refresh
pins, and all four M12a regression pins. The measurement harness in debug
(numbers above) and in release, twice for the embedded legs. The full suite in a
clean env, clippy, fmt, file-size count. The demo smoke in a pty (renders,
exits 0 on `Esc`). The lambo pin re-confirmed from `Cargo.lock` and
`insert_concept`'s replace-by-id + per-turn-`Derives` behaviour read from the
pinned tree at `4c6fc93` to verify the harness's shape claim. Every hunk of the
full diff read against the current tree; `of_memory`/`of_graph` callers
enumerated by grep.

**Read, not executed.** The reversed-order contention race itself (mutations (a)
and (b) establish the pins' failure modes textually; the round-2 race already
demonstrated the wedged behaviour on this machine and nothing about the lock
changed). The `--demo` pty interplay of a key typed into the draft (focus is on
the conversation; `Esc` quits — exercised). The non-unix stubs, as in prior
rounds. The doc's 18.2 ms comparison (my 4k build-only run is 24.0 ms; two runs
of the release embedded leg bracket the doc's number, so the gap is
code-change-plus-variance, not a measurement error in the harness).

## Notes for M12c

* The embedded legs of the tick test are unasserted and dominate its ~13.5 s
  debug runtime (~7 s of it). They are what keeps the doc's embedded numbers
  honest — but if the suite ever needs trimming, the embedded legs (or the
  sample count) are the first candidate, or the assert could be extended to the
  release shape the product actually ships.
* The debug embedded 4k number (~1.8 s/rebuild) confirms the doc's note: the
  fold's scalar cosine is the first thing a faster tick would look at. At a
  year's turns (~20k) even the release embedded leg would approach the budget;
  M12c's consolidation is the natural place to revisit it.
* The figures pin is sound in a way the copy pin is not: it pins evaluation
  *order*, which source order fully determines. Any future tightening of the
  copy pin should copy its reasoning — the fix in M12b-R1-1 (require the block
  close between copy and build) preserves the existing assert's shape.
* `PLAN.md`'s M12b bullet now carries the guard-duration item and points at the
  pins; after M12b-R1-1 it should read "pinned" only once the block-close
  assert lands.
