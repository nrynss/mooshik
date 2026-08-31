# M12b round 8 — final confirming review of the round-7 remediation (binding-position count)

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1 through round-7 remediations (12 modified + 13 untracked — the
full expected set, verified identical before and after every mutation, now with
this record beside them). Scope, per the orchestrator's Option-B decision: confirm
that the round-7 remediation is genuine (an acquisition counts only as the
initializer of a local binding), that the pin's doc names exactly what the checks
prove and what they do not (the two documented limits: out-of-body indirection and
value-escape of the bound guard, both framed as deliberate sabotage, R2-2
precedent), that the milestone holds, and that zero residue remains within the
documented limits. Verify, don't re-litigate: the battery below is the spot-check
battery the orchestrator prescribed, not a fresh 45-form campaign — every form it
re-runs is drawn from the round-6/round-7/remediation tables. All transient
mutations reverted and `sha256sum`-verified byte-identical to the pre-mutation
state after every run (`src/memory/view.rs`
`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the same hash
every prior round recorded — the round-7 remediation changed `view.rs` not at all,
and neither did this round); `git status --porcelain` shows exactly the same 12
modified + 13 untracked as before I started (plus this record), nothing committed.
Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so every `cargo` invocation ran
under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**APPROVE** — zero findings. M12b is clean within the documented limits and ready
to commit.

The R7 fix is genuine, at the code and at the behavior: `AcquisitionHunter`
counts an acquisition only when it is the initializer of a `syn::Stmt::Local`
(`visit_stmt` fires the count solely on `Stmt::Local` whose `init` — after the
existing `unwrap`, which peels parens, groups, references, derefs, casts,
single-expression blocks and now single-expression `unsafe` blocks — satisfies
`is_acquisition`), so an acquisition consumed as a call argument is never the
guard. Executed: the unbound form `std::mem::forget(memory.graph().read());`
inside the copy block fails the count with `left: 0, right: 1` at 331:5 — the
reviewer's prescription, verbatim class. The bound-then-forgotten form
(`let guard = ...; std::mem::forget(guard);`) passes **by design** with zero
warnings — the count sees the binding, and per Option-B the value-escape of the
bound guard is the second documented limit, named in the doc with the same status
as the round-6 out-of-body indirection limit. `drop(guard)` passes, zero warnings
(a release, not an escape — no false positive). Every one of the nine executed
battery forms behaved exactly as the remediation record claims: 0 mismatches.
The doc's check-4 claim is now literally true for every form the pin passes — the
guard the count sees is a bound local whose binding scope is the block — and the
round-7 finding's quoted false claim ("is the guard that drops at the block's
close") is gone. The two documented limits are framed honestly as deliberate
sabotage, R2-2 applies to both, and the milestone and all gates hold. One wording
nit from round 7 (the doc's "the body then contains no acquisition tokens at all"
clause) persists un-remediated; it was graded "not a finding" in round 7 and
remains one — see the doc-honesty section below.

## Findings

None.

## The R7 fix is genuine — spot-check battery

The pin under review: `the_build_runs_against_the_copy_and_not_the_guard` in
`src/memory/view_session_tests.rs` (lines 220–348), parsing `of_memory`'s body
slice with `syn` and asserting: (1) no `Expr::Macro` anywhere (253:5); (2) the
copy is the unique top-level `let graph = { … }` (263:61 expect, 268:5
uniqueness), `let stats = memory.stats();` precedes it (275:57, 279:5), and a
top-level `of_graph(…)` follows it (284:57); (3) confinement — no expression
outside the copy's block references `memory` (308:9); (4) exactly one
graph-guard acquisition, inside the copy's block, **and it is the initializer of
a local binding** (331:5 count, 343:5 in-block count) — the R7 change, with the
`unwrap` unsafe-block peel (lines 462–468) keeping the round-7 safe form (u1)
counting. The doc comment (lines 148–219) states the structural rule in full and
names the two documented limits.

Every mutation transient; `view.rs` restored from a byte copy and
`sha256sum`-verified identical (`c30de825…`) after each run; all runs in a clean
env (the three DSN variables unset).

| Form | Pin | Result |
| --- | --- | --- |
| (d) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — baseline before the battery and again via the full-suite run on the restored bytes at the end: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out`, zero warnings |
| (a) **unbound consumption**: `std::mem::forget(memory.graph().read());` + `let data = ViewData::from_graph(&memory.graph().read());` inside the block (acquisitions consumed as call arguments, never bound) | same | **caught** — `panicked at src/memory/view_session_tests.rs:331:5:` `assertion `left == right` failed`, **left: 0, right: 1**, zero warnings — the reviewer's prescription is real: an acquisition that is not a `let` initializer is invisible to the count (the `Box::leak`/`ManuallyDrop` argument spellings are the same class, per the remediation record's executed v2) |
| (b) **bound-then-forgotten** (the round-7 finding's executed form): `let guard = memory.graph().read(); let data = ViewData::from_graph(&guard); std::mem::forget(guard); data` inside the block | same | **passes** — `ok. 1 passed`, zero warnings — the **documented limit**, exactly as the doc names it ("`std::mem::forget(guard)` … after the binding consumes the value and never runs `Drop`, so the read lock is held across the build and forever — while the count still sees the binding and passes"), Option-B status, R2-2 |
| (c) **bound-then-dropped**: same shape with `drop(guard);` | same | **passes** — `ok. 1 passed`, zero warnings — `drop` is a release, not an escape: no false positive |
| (e1) **confinement form** (m8): `let guard = (memory.graph()).read();` after the block | same | **caught** — `panicked at …view_session_tests.rs:308:9:` `no expression outside the copy's block may reference the `memory` parameter: …`; one `unused variable: guard` warning |
| (e2) **macro form** (o2): module `macro_rules! grab` in `view.rs` + `let inner = grab!(memory);` inside the copy block | same | **caught** — `panicked at …view_session_tests.rs:253:5:` `no macro invocation may appear in of_memory's body: …`; one `unused variable: inner` warning (M12b-R5-1 stays closed) |
| (e3) **flat form** (b): `let guard = memory.graph().read(); let graph = ViewData::from_graph(&guard); of_graph(...)` | same | **caught** — `panicked at …view_session_tests.rs:263:61:` `the copy is a block: …`; zero warnings (round-1 class stays closed) |
| (f1) **safe nested guard scope** (i1): copy wrapped in one extra `{ }` layer | same | **passes** — `ok. 1 passed`, zero warnings |
| (f2) **safe string-brace** (f4): `let _s = "{";` inside the copy block | same | **passes** — `ok. 1 passed`, zero warnings |

Nine executed forms (baseline + 8 spot-checks), 9/9 expected outcomes, 0
mismatches. The two historical-evasion families not re-run here (the full
round-6 receiver-respelling nine and the alias-map/closure/whitelist/slice
probes) are covered by the round-6 and round-7 records' executed tables at the
same check anchors, and the R7 remediation's own 45-row battery re-ran all of
them against the new count; nothing in the binding-position rework touched those
checks' code paths except the count and the `unwrap` unsafe peel, both exercised
above.

## The doc is honest

The pin's full doc comment (lines 148–219) was re-read sentence by sentence. It
states the structural rule exactly as the checks implement it: (1) no
`Expr::Macro` anywhere in the fn (lines 158–162); (2) the copy is one top-level
`let graph = { … }` statement, the only statement of the body binding a block to
`graph`, the whitelisted `let stats = memory.stats();` precedes it, a top-level
`of_graph(…)` follows it (lines 163–169); (3) confinement — no expression outside
the copy's block may reference `memory`, the single whitelisted exception named
(lines 170–181); (4) exactly one graph-guard acquisition, inside the copy's
block, **and it is the initializer of a local binding**, so "the guard the count
sees is a bound local whose binding scope is the block" (lines 182–194). It names
the two documented limits — out-of-body indirection, "a helper that returns the
guard, or a caller that takes the copy with it" (lines 205–209), and value-escape
of the bound guard, `std::mem::forget(guard)` / `Box::leak(Box::new(guard))` /
`ManuallyDrop::new(guard)` after the binding, "the read lock is held across the
build and forever — while the count still sees the binding and passes" (lines
210–213) — both framed as "two deliberate sabotage classes, both changes an
author makes on purpose rather than a natural refactor of the shipped shape"
(lines 205–207), with the R2-2 precedent cited for both (lines 216–219). The
false claim the round-7 finding quoted ("Exactly one, inside the block, is the
guard that drops at the block's close") is gone; check 2's doc (lines 166–169)
carries the same caveat ("a deliberate value-escape of it is the second
documented limit below").

One imprecision persists, exactly the round-7 wording nit, still not a finding:
line 209's rationale "because the body then contains no acquisition tokens at
all" is false in the **additive** variant of the out-of-body limit — the executed
round-7 (g1) form keeps the shipped in-block acquisition and adds
`let guard = global_guard();` after the block, so the body still contains
acquisition tokens while the pin passes (the count sees the real in-block
binding; `global_guard()` is not an acquisition). The sentence's subject is the
class where the acquisition moves out **entirely** — in which the clause is true —
and the class itself is named without qualification ("a helper that returns the
guard"), so the boundary is drawn correctly and no reader is misled about what
the pin proves or what passes. Round 7 graded this "a wording note for the
remediator, not a finding", and the Option-B decision closed the cycle at the
documented limits; it does not overclaim the boundary and is not re-found here.
Everything else in the doc claims exactly what the checks prove.

## The milestone holds

`src/memory/view.rs` byte-identical to `c30de825…` (975 lines) before, throughout
and after the battery — the R7 remediation and this round changed it not at all;
`of_memory` holds the guard for exactly one copy. `Cargo.lock` carries
`syn 2.0.119` (one root dependency line; already vendored transitively by
serde_derive/schemars, so no new package enters the tree) and lambo stays pinned
at `4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`; `cargo test --locked` resolves and
runs. All seven milestone pins green on the final tree, individually:
`a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
`a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
`the_figures_are_read_before_the_graph_guard` (the three M12b pins) and
`the_local_database_is_created_and_repaired_private`,
`the_scratch_sandbox_and_script_stay_private`,
`two_sandboxes_opened_in_the_same_instant_are_two_directories`,
`a_termination_signal_disposition_is_restored_after_the_session` (the four M12a
regression pins) — each `test result: ok. 1 passed; 0 failed; 545 filtered out`.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset), on the final tree:

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.51 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  795, `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs`
  169, `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs`
  119, `cli/tests.rs` 811, `PLAN.md` 735, `Cargo.toml` 62 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`);
  syn 2.0.119 confirmed in `Cargo.lock`.
* The seven milestone pins green on the final tree, individually (listed under
  The milestone holds); the pin under review green on the shipped bytes before
  the battery and again inside the full-suite run after it.
* `src/memory/view.rs` byte-identical to the recorded hash (`c30de825…`) at the
  end; `git status --porcelain` shows exactly the pre-review set (12 modified +
  13 untracked) plus this record; nothing committed.

## What was executed vs. only read

**Executed.** The nine-form spot-check battery (baseline shipped form, the
unbound-consumption form, the bound-then-forgotten form, the bound-then-dropped
form, the confinement form, the macro form, the flat form, the nested-scope form,
the string-brace form) — every mutation applied to a byte copy-restored `view.rs`
and `sha256sum`-verified identical (`c30de825…`) before and after every run; full
panic locations re-captured for the caught forms (331:5, 308:9, 253:5, 263:61);
warning counts for every run; the compile of every mutation verified (each
mutation must compile — the whole test target builds); the `AcquisitionHunter`
binding-position rule and the `unwrap` unsafe-block peel confirmed in the source
(lines 665–676 and 462–468); the doc comment read in full (lines 148–219) and
checked clause by clause against the implemented checks; the seven milestone pins
individually; the full suite in a clean env; clippy; fmt; the file-size count;
the lambo pin and the syn version re-confirmed from `Cargo.lock`.

**Read, not executed.** The `Box::leak`/`ManuallyDrop` argument spellings of the
unbound form (same class, same mechanism — the count sees no `Stmt::Local` init —
the executed `std::mem::forget` argument form stands for them; the remediation
record executed v2 as the `forget` form with the same `left: 0, right: 1`). The
additive out-of-body variant (g1) and the full historical battery (round-6's
45-row and remediation's 45-row executed tables, all at the same check anchors;
the binding-position rework touched only the count and the `unwrap` peel, both
exercised here). The reversed-order contention and writer-starvation races
themselves (no lock code changed in this remediation; rounds 1–3 demonstrated the
wedged behaviour on this machine). The `--demo` pty interplay (round 1 executed
it; this round touches no TUI code).

## Closing statement

**M12b is clean within the documented limits and ready to commit.**

The round-7 remediation is genuine: the count now proves the guard is a bound
local — an acquisition counts only as the initializer of a `Stmt::Local`, so the
unbound consumption class (forget-as-argument) fails the pin, the bound-then-
forgotten value-escape passes only as the documented limit the doc names, and
`drop(guard)` and every safe form pass without false positives. The doc is honest
about what the checks prove and what they do not, naming both limits — out-of-body
indirection and value-escape of the bound guard — as deliberate sabotage with the
R2-2 precedent, and the check-4 claim that was false in round 7 is gone. The
milestone holds: `view.rs` byte-identical to `c30de825…`, all seven pins green,
all gates green, lambo pinned at `4c6fc93`, syn locked at 2.0.119, and the tree
carries exactly the implementation + seven remediations + this record, nothing
committed. Zero findings, zero residue within the documented limits. M12b closes;
M12c may proceed.
