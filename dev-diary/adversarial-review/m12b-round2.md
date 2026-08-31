# M12b round 2 — adversarial re-review of the round-1 remediation

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1 remediation (10 modified + 3 untracked — the full expected set,
verified identical before and after every mutation). Scope: the two round-1
findings (P2 M12b-R1-1, P3 M12b-R1-2) and the milestone's continuing hold. All
transient mutations reverted and `sha256sum`-verified identical to the
pre-mutation state (`view.rs` `c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`,
which matches the remediation record's own pre-mutation hash byte-for-byte);
`git status --porcelain` shows exactly the same 10 modified + 3 untracked as
before I started, now with this record beside them. Nothing committed. Ambient
shell exports a live `LAMBO_POSTGRES_DSN`, so every `cargo` invocation ran under
`env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 1 × P2.

The round-1 remediation is genuine and verified: the flat three-statement form
and the inline-in-call form both fail the widened pin with exactly the quoted
messages, the shipped block-scoped form passes, the doc sentence is now
like-for-like and reproduces against my own runs, and the milestone still holds
(tick rebuilds, a write appears without a keystroke, the budget fits with the
claimed margins, the R1-3 structural close stands, all four M12a regression pins
pass). What survives is the same class of hole round-1 flagged as P2: the
widened pin still cannot see the guard's **scope**. Hoisting the guard binding
one line above the copy block —

```rust
let guard = memory.graph().read();
let graph = { ViewData::from_graph(&guard) };
of_graph(&stats, &graph, now)
```

— satisfies the order assert *and* the new block-close check while the
`RwLockReadGuard` (bound at function scope, dropped at the end of `of_memory`,
never at its last use) stays held for the entire build, and the pin passes:
executed, one run, `ok`. The pin's own doc — "with the copy's block closed
before the build call, because that is the order the code must be written in
for the guard to be gone by the time the build runs" — is false for that form,
and the remediation record's claim that the check is "true of the shipped shape
and of nothing the pin accepts" overstates it in the same way. By the round-2
precedent that made round-1's finding a P2 (a pin passing with the hazard
present), this is a P2, and per the round's instruction there are no deferrals.

## What held up under attack

* **The two documented mutations fail exactly as quoted.** Flat form →
`panicked at src/memory/view_session_tests.rs:188:10: the copy's block closes
before the build` (the remediation's verbatim message); inline-in-call form →
`panicked at src/memory/view_session_tests.rs:177:5: of_memory must copy the
graph out under the guard and build from the copy: the current order holds the
guard across the build, which starves a writer at a 250 ms tick` (round-1's
verbatim message). Shipped form green. Each mutation reverted and hash-verified.
* **The pin's doc now names the flat-form hazard accurately.** "A flat
three-statement form reads in that order yet keeps the guard alive to the end of
`of_memory`'s scope, so the missing block close is what the check bites on" is
exactly what the check sees — for the flat form. The residual overclaim is the
"must be written in" clause (see the finding): it asserts more than a textual
check can prove.
* **The doc sentence is now like-for-like and reproduces.** The M12a records
(`m12a-round2.md:77-79`) measured **6.5 ms / 18.2 ms** as the whole pass at the
two shapes — which is the pre-copy build, since M12a built under the guard; the
reworded sentence says exactly that. My debug harness runs (twice, unembedded
legs): whole means **7.19 / 8.54 ms** (1k) and **26.97 / 28.13 ms** (4k) — the
sentence's ~8 ms / ~29 ms bracket — of which the copy means are **0.55 / 0.65 ms**
and **3.02 / 3.88 ms** — its ~0.6 ms / ~3.5 ms bracket. Budget: 250 / 27 ≈ 9.3×
at the 4k shape, the claimed ~9× margin. Release: unembedded whole means
**0.83 / 3.10 ms** — the doc's ~0.9 / ~2.9. The sentence reproduces.
* **The milestone holds.** `a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`
(real sqlite `Memory`, the write lands in the trickle without a keystroke),
`a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
`the_figures_are_read_before_the_graph_guard`,
`a_refresh_onto_a_smaller_model_does_not_break_the_next_draw`, and all four M12a
regression pins (`the_local_database_is_created_and_repaired_private`,
`the_scratch_sandbox_and_script_stay_private`,
`two_sandboxes_opened_in_the_same_instant_are_two_directories`,
`a_termination_signal_disposition_is_restored_after_the_session`) — green on
the remediated tree.
* **The R1-3 structural close stands.** `fn of_graph(stats: &MemoryStats,
graph: &ViewData, now)` is private; grep over `src/` shows its only non-test
caller is `of_memory`, and `of_memory`'s only production callers are
`tui_cmd::live`'s two (first model + tick closure). The hazard round-1
identified was purely the guard's lifetime — which is exactly what makes the
pin's residual blind spot the finding it is.
* **No behaviour change beyond the findings.** Line counts match the
remediation record's deltas exactly: `view.rs` 974 → 975 (the reworded doc
paragraph gained one line), `view_session_tests.rs` 299 → 308 (+3 doc lines,
+6 check lines). Everything else in the tree is the round-1-reviewed M12b
implementation; every hunk re-read against the current tree.
* **Gates and pin.** Full suite in a clean env, clippy, fmt, file-size caps,
lambo pinned at `4c6fc93` — all as recorded below.

## Findings

### P2

**M12b-R2-1 — the widened pin still cannot see the guard's scope: a one-line
hoist of the guard binding above the copy block passes with the guard held
across the build.**

The round-1 fix makes the pin require a `}` between the copy and the build call,
which catches the flat three-statement form. It does not catch the same hazard
re-introduced by hoisting the guard binding out of the block:

```rust
let stats = memory.stats();
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

Here `guard` is bound at function scope and dropped only at the end of
`of_memory` — after `of_graph` returns — so the `RwLockReadGuard` is held for
the whole build, the writer starvation at a 250 ms tick the copy exists to
prevent, reintroduced by moving one line up two lines. The pin accepts it:
`guard < copy < build` holds and `body[copy..build]` contains the block's `}`.
Executed: the mutation passes the pin (`ok`, one run). The pin's doc — "with the
copy's block closed before the build call, because that is the order the code
must be written in for the guard to be gone by the time the build runs" — is
false for this form, and the remediation record's "true of the shipped shape and
of nothing the pin accepts" is the same overclaim the round-1 review called the
core defect.

*Remediation.* Assert the copy block opens before the guard is taken, in the
same string-anchored style the pin already uses — e.g. after the existing
checks:

```rust
    let block = body
        .find("let graph = {")
        .expect("the copy is a block, opened before the guard");
    assert!(
        block < guard,
        "the guard must be taken inside the copy's block so it drops before the build"
    );
```

The shipped shape (`let graph = { let guard = …; … };`) passes; the hoisted form
fails the assert; the flat form already fails the block-close expect. The check
then enforces the shipped discipline end to end — guard inside the copy's block,
block closed before the build — which is what the pin's doc already claims it
proves.

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the
remediation record's own hash).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) flat three-statement form — guard alive across the whole build | `the_build_runs_against_the_copy_and_not_the_guard` | **caught** — `the copy's block closes before the build`, verbatim vs the remediation record |
| (b) build fed `ViewData::from_graph(&guard)` inside the call | same | **caught** — round-1 verbatim starvation message, byte-for-byte |
| (c) shipped block-scoped form | same | **passes** — twice: before the mutations and after the final revert |
| (X) guard binding hoisted above the copy block, block retained | same | **SURVIVES** — 1 run, 1 pass (P2, M12b-R2-1) |

Each mutation reverted and hash-verified identical; the shipped-form runs came
from the restored bytes. The doc-comment review: the pin's doc accurately
describes the flat-form hazard and what the block-close check bites on; its
"that is the order the code must be written in for the guard to be gone by the
time the build runs" clause is what the X mutation falsifies.

## Gates

Run by me at the end, in a clean env (all three DSN variables unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`) **+ 0 doc**, exit 0. Matches both prior records'
  numbers exactly. Full run ~5 min wall; the lib phase 14.34 s of which the
  tick measurement test is ~13.2 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975, `view_session_tests.rs` 308,
  `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs` 169,
  `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs` 119,
  `cli/tests.rs` 811, `PLAN.md` 735 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`).

## What was executed vs. only read

**Executed.** All four mutations (a, b, c, X) against the widened pin, each
reverted and `sha256sum`-verified, with the pin run before and after. The
headline milestone pins (`a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
`a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`), the figures
pin, the refresh pin, and all four M12a regression pins. The measurement
harness in debug twice (unembedded and embedded legs, numbers above) and in
release once (0.83 / 3.10 ms unembedded; 28.1 / 136.3 ms embedded — the 4k
embedded leg read round-1's documented cold value). The full suite in a clean
env, clippy, fmt, file-size count. The lambo pin re-confirmed from
`Cargo.lock`. The M12a records' 6.5 / 18.2 ms read from `m12a-round2.md` as the
comparison baseline. Every hunk of the full diff read against the current tree;
`of_graph`/`of_memory` callers enumerated by grep.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations (a),
(b) and (X); round 1 already demonstrated the wedged behaviour on this machine
and no lock code changed). The `--demo` pty interplay (round 1 executed it;
nothing in the remediation touches the demo path). The non-unix stubs, as in
prior rounds. The release embedded 4k leg's clean-run value (~108 ms): round-1
measured 109.7 ms twice and 135 ms cold; my 136.3 ms is the documented cold
value, so the doc's ~108 stands on round-1's clean runs — noted for M12c rather
than raised as a finding, since the remediation did not touch that paragraph and
the budget claim (250 ms) holds either way.

## Notes for M12c

* The copy pin needs one more assert to be what its doc claims: the copy block
  must **open** before the guard (see M12b-R2-1's remediation). The figures pin
  is the model — it pins evaluation order, which source order fully determines;
  the copy pin pins a lifetime, which no purely textual check can exhaust, so
  the doc should claim exactly what the two anchored markers prove.
* The release embedded 4k leg is variable enough that the doc's "~2.3× margin"
  measured 1.8× this round (136.3 ms vs round-1's 109.7 clean). The
  product-vectors paragraph's numbers are the first thing to re-verify in M12c;
  at a year's turns even the release embedded leg approaches the budget.
* The embedded legs of the tick test are unasserted and dominate the lib phase
  (~13.2 s of 14.34 s). Same candidate as round 1 if the suite ever needs
  trimming.
* `PLAN.md`'s M12b bullet says the guard-duration item is "pinned by the
  guard-duration pins"; after M12b-R2-1 lands, that is true only once the
  block-open assert is in.
