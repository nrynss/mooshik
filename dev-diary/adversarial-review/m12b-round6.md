# M12b round 6 — adversarial re-review of the round-5 remediation

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1 through round-5 remediations (10 modified + 11 untracked — the
full expected set, verified identical before and after every mutation, now with
this record beside them). Scope: the two round-5 findings (P2 M12b-R5-1 — the
pin did not fail closed on macro invocations; P2 M12b-R5-2 — the
no-acquisition-after-close check was fail-open to whitespace inside the call),
the documented remaining frontier (`(memory.graph()).read()`), and the
convergence judgment: has the text-anchored pin converged, or is the token-level
(syn) pin the honest end-state. All transient mutations reverted and
`sha256sum`-verified byte-identical to the pre-mutation state after every run
(`src/memory/view.rs` `c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`,
the same hash every prior round recorded — the round-5 remediation changed
`view.rs` not at all); `git status --porcelain` shows exactly the same 10
modified + 11 untracked as before I started, nothing committed. Ambient shell
exports a live `LAMBO_POSTGRES_DSN`, so every `cargo` invocation ran under
`env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 1 × P2, 1 × P3.

The round-5 remediation is genuine as far as it goes: M12b-R5-1 and M12b-R5-2
are closed at their site — every macro invocation in the flattened body fails
closed at the first check, and every acquisition is matched modulo insignificant
whitespace (six executed forms, all caught, detailed below). What survives is
the class the round-5 record itself named as the remaining frontier — the
parenthesized receiver — plus eight further compiling spellings of the same
hazard that this round executed. All nine bind a function-scope
`parking_lot::RwLockReadGuard` after the copy's block close and hold it across
the whole `of_graph` build; the pin passes each with `ok. 1 passed`. That is the
same verdict trigger every prior round used — a pin passing with the hazard
present is a P2, no deferrals — and the R2-2 precedent for accepting a
documented pin limit does not cover it (see Findings). The text-anchored
approach has not converged: this round found nine holes at once in a class
(receiver expressions and indirection) that is unbounded, and the honest
end-state is the token-level pin the reviewer's own records named. Concrete
prescription below; not implemented (the cycle's remediation does).

## Findings

### P2

**M12b-R6-1 — the receiver-respelling and indirection family passes the pin
with the guard held across the build; the documented parens-receiver frontier
is one of nine.**

The round-5 fix closed the macro class and the whitespace class; the
parenthesized-receiver spelling the round-5 record named as the remaining
textual frontier is confirmed to pass — and beyond it, eight more compiling
spellings pass, every one binding a guard at function scope after the copy
block's close and holding it across `of_graph`. All executed on the shipped
`view.rs` (each reverted and hash-verified after the run):

* (m8) `let guard = (memory.graph()).read();` after the block — **passes**
  (the documented frontier, confirmed exactly as the round-5 record says).
* (m8b) `let guard = (memory.graph()).read();` hoisted above the block —
  **passes**.
* (m9) `let guard = Memory::graph(&memory).read();` after the block — UFCS on
  the `&self` method (confirmed `pub fn graph(&self) -> &Arc<RwLock<Graph>>`
  at lambo `4c6fc93` src/memory.rs:1414) — **passes**.
* (m10) `let g = &memory; let guard = g.graph().read();` after the block — a
  binding alias — **passes**.
* (m12) `let guard = parking_lot::RwLock::read(memory.graph());` after the
  block — UFCS on the lock's `read` (deref-coerces the `&Arc<RwLock<Graph>>`)
  — **passes**.
* (m13) `let guard = memory.graph().try_read().unwrap();` after the block — a
  read-family method whose text the exact spine misses (`graph().try_read()`
  does not contain `graph().read()`) and which acquires the guard even under a
  queued writer — **passes**.
* (m14) `let guard = (*memory).graph().read();` after the block — **passes**.
* (m15) `let guard = { memory }.graph().read();` after the block — **passes**.
* (m18) module-level `fn read_lock(m: &Memory) -> parking_lot::RwLockReadGuard<'_, Graph>`
  + `let guard = read_lock(&memory);` after the block — the helper sits above
  the pin's slice, exactly like the round-5 `macro_rules!` did — **passes**.

The pin's doc claim — "With macro invocations rejected and the whitespace
flattened, every acquisition text is visible to those checks, so together they
put every acquisition inside the copy's block" — is false for this class: the
receiver expression (parens, deref, block, cast, UFCS path, alias) and plain
call indirection are text the spine `memory.graph().read()` cannot see, and the
class is unbounded — no finite set of textual anchors covers it. The turbofish
spelling the review brief named is the one form that does not compile:
`memory.graph().read::<()>()` fails with `error[E0107]: method takes 0 generic
arguments` (`parking_lot`'s `read` has no type parameters), so it is not a
residual. `self.graph().read()` is not applicable: `of_memory` is a free
function with no `self` receiver (checked). Every prior round judged "a pin
passing with the hazard present" a P2 with no deferrals; the R2-2 precedent for
accepting a documented limit does not apply here — that limit (m12a round 3:
"the R2-2 pin tests `name` directly, not `create`'s wiring to it… the fault the
pin guards is the name format") sat **outside** the named fault, while every
form above is the named fault exactly: a graph guard bound at function scope
after the copy's block and held across the build, the writer starvation the
copy exists to prevent.

*Remediation sketch.* The token-level pin below (Convergence judgment), which
defines the acquisition structurally instead of by text.

### P3

**M12b-R6-2 — the no-macro check also rejects legitimate `!(…)` unary-not
expressions; a dormant false positive on future code.**

`!(x)` parses as unary not, not a macro invocation, but the check fires on the
flattened `!(` window: executed `let _b = !(1 > 2);` inside the body fails the
pin at `218:5` with the macro message. Zero impact on the shipped body (verified
the slice contains no `!` at all), so the false positive is dormant — but a
future legitimate `!(…)` or `!{…}` expression in `of_memory` is rejected with a
misleading message, and the doc's "Every macro invocation spells `!`
immediately followed by `(`, `{` or `[`" is true only one-way. The structural
`Expr::Macro` rejection in the token-level pin closes it as a side effect (a
unary not parses as `Expr::Unary`, never `Expr::Macro`).

*Remediation sketch.* Reject macro invocations in the AST (`Expr::Macro` walk)
instead of the text `!(`-followed-by-delimiter window.

## What held up under attack

* **M12b-R5-1 is genuinely closed at its site.** All three executed forms fail
  closed at the first check, `src/memory/view_session_tests.rs:218:5` — "no
  macro invocation may appear in of_memory's body…" — with the guard warning
  present in each: (m1) module-level `macro_rules! grab` + `let guard =
  grab!(memory);` after the block's close (the round-5 finding, reproduced);
  (m2) `let guard = grab ! (memory);` (spaced — flattens to `grab!(memory)`),
  caught; (m3) `let guard = grab!(memory);` inside the copy block (would expand
  to a *safe* in-block acquisition — rejected anyway, the documented coarse
  stance), caught. The `windows(2)` check on the flattened text covers every
  function-like macro invocation spelling: the three delimiter forms, whitespace
  between `!` and the delimiter, and comments between them are all gone from the
  flattened text, so `vec![…]`, `println!(…)`, `format!(…)`, `foo !{…}` and
  `foo!\n(…)` all spell `!` immediately followed by `(`/`{`/`[`. The shipped
  body has zero `!` (verified), so the check costs nothing on it. A
  `macro_rules!` *definition* inside the body would not trip the `!`-window (its
  `!` is followed by an identifier), but a definition without an invocation
  cannot acquire a guard, and an invocation is always caught — the class is
  closed.
* **M12b-R5-2 is genuinely closed at its site.** All four whitespace forms
  fail: (m4) `memory.graph().read ();` after the close → `300:5` ("no graph
  guard may be taken after the copy's block closes…"), verbatim vs the round-5
  record; (m5) `memory\n.graph()\n.read();` after the close → `300:5`; (m16)
  the linebreak inside the call parens, `memory.graph().read\n();` → `300:5`;
  (m7) the spaced hoisted guard above the block → `288:5` containment ("the
  guard must be taken inside the copy's block so it drops before the build"),
  found by the guard anchor on the flattened text rather than panicking the
  expect — the round-5 claim holds. (m6) `unsafe { memory.graph().read() };`
  after the close → `300:5` with the `unnecessary unsafe` warning, as recorded.
* **The round-1 through round-4 classes stay closed.** The shipped form passes
  twice (baseline before the mutations and after the final revert, `ok. 1
  passed; 545 filtered out`), and the check chain — no-macro `218:5`, block-close
  `245:10`, depth `267:5`, containment `288:5`, no-acquisition `300:5` — is
  unchanged from the round-5 record; the forms that exercise those checks
  (flat, hoisted, decoys, nested scopes) are pinned by the same anchors on the
  same flattened text this round re-verified end to end.
* **The milestone still holds.** `view.rs` is byte-identical to the hash every
  prior round recorded (`c30de825…`, 975 lines); `of_memory` holds the guard
  for exactly one copy; all seven pins green, individually: the three M12b pins
  (`a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
  `a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
  `the_figures_are_read_before_the_graph_guard`) and the four M12a regression
  pins (`the_local_database_is_created_and_repaired_private`,
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`) — each
  `test result: ok. 1 passed; 0 failed`.
* **Gates.** Full suite in a clean env, clippy, fmt, file-size caps, lambo pin
  — all as recorded below.

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). All runs under a clean env (the three DSN variables unset). The
pin run on the shipped bytes before the mutations and again after the final
revert.

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test result: ok. 1 passed; 0 failed; 545 filtered out` |
| (m1) module-level `macro_rules! grab` + `let guard = grab!(memory);` after the block (M12b-R5-1 reproduced) | same | **caught** — `panicked at src/memory/view_session_tests.rs:218:5: no macro invocation may appear in of_memory's body: an invocation's expansion is invisible to these source-order checks, so it could acquire the graph guard at function scope and hold it across the build`; one `unused variable: guard` warning |
| (m2) `let guard = grab ! (memory);` after the block (spaced) | same | **caught** — `grab ! (memory)` flattens to `grab!(memory)`: same 218:5 message; one warning |
| (m3) `let guard = grab!(memory);` inside the copy block | same | **caught** — macro tokens anywhere fail closed, even one expanding to a safe in-block acquisition: same 218:5 message; one warning |
| (m4) `let guard = memory.graph().read ();` after the block (M12b-R5-2 reproduced) | same | **caught** — the flattened window contains the acquisition: `panicked at src/memory/view_session_tests.rs:300:5: no graph guard may be taken after the copy's block closes: a guard bound there is outside the block and held across the build`, verbatim vs the round-5 record; one warning |
| (m5) `let guard = memory\n.graph()\n.read();` after the block | same | **caught** — multiline spine flattens to the exact text: same 300:5 message; one warning |
| (m6) `let guard = unsafe { memory.graph().read() };` after the block | same | **caught** — exact text inside the unsafe block: same 300:5 message; two warnings (`unnecessary \`unsafe\` block`, `unused variable: guard`) |
| (m7) `let guard = memory.graph().read ();` hoisted above the block | same | **caught** — spaced acquisition found by the guard anchor (no expect panic) and fails containment: same 288:5 message, one warning |
| (m16) `let guard = memory.graph().read\n();` after the block (linebreak inside the call parens) | same | **caught** — same 300:5 message; one warning |
| **(m8) `let guard = (memory.graph()).read();` after the block — the documented parens-receiver frontier** | same | **SURVIVES** — `test result: ok. 1 passed; 0 failed; 545 filtered out`, one warning; the extra parens break the exact spine even on the flattened text. Confirmed exactly as the round-5 record says (P2, M12b-R6-1) |
| **(m8b) `let guard = (memory.graph()).read();` hoisted above the block** | same | **SURVIVES** — `ok. 1 passed`; guard held across the build from before the copy block (P2, M12b-R6-1) |
| **(m9) `let guard = Memory::graph(&memory).read();` after the block** | same | **SURVIVES** — `ok. 1 passed`; UFCS on the `&self` method (P2, M12b-R6-1) |
| **(m10) `let g = &memory; let guard = g.graph().read();` after the block** | same | **SURVIVES** — `ok. 1 passed`; binding alias (P2, M12b-R6-1) |
| (m11) `let guard = memory.graph().read::<()>();` after the block (turbofish) | same | **does not compile** — `error[E0107]: method takes 0 generic arguments but 1 generic argument was supplied`; `read` has no type parameters, so this spelling cannot exist; not a residual |
| **(m12) `let guard = parking_lot::RwLock::read(memory.graph());` after the block** | same | **SURVIVES** — `ok. 1 passed`; UFCS on the lock, deref-coercing the `&Arc<RwLock<Graph>>` (P2, M12b-R6-1) |
| **(m13) `let guard = memory.graph().try_read().unwrap();` after the block** | same | **SURVIVES** — `ok. 1 passed`; the read-family method text (`graph().try_read()` does not contain `graph().read()`) acquires the guard even under a queued writer (P2, M12b-R6-1) |
| **(m14) `let guard = (*memory).graph().read();` after the block** | same | **SURVIVES** — `ok. 1 passed` (P2, M12b-R6-1) |
| **(m15) `let guard = { memory }.graph().read();` after the block** | same | **SURVIVES** — `ok. 1 passed` (P2, M12b-R6-1) |
| **(m18) module-level `fn read_lock(m: &Memory) -> parking_lot::RwLockReadGuard<'_, Graph>` + `let guard = read_lock(&memory);` after the block** | same | **SURVIVES** — `ok. 1 passed`, one warning; the helper sits above the pin's slice, exactly like the round-5 `macro_rules!` — the indirection class (P2, M12b-R6-1) |
| (m17) `let _b = !(1 > 2);` inside the body (legitimate unary not) | same | **caught** — `!(` matches the macro window: same 218:5 message; a dormant false positive on valid non-macro code (P3, M12b-R6-2) |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. Nine survivors, nine caught
(including the m17 false positive), one compile error — every survivor is a
compiling program that binds a function-scope guard and holds it across the
build.

## Convergence judgment

**The text-anchored pin has not converged, and the evidence is this round's
table.** The sequence of classes, closed then re-expressed: round 1 flat form →
round 2 comment/literal anchors → round 3 guardless decoy → round 4 nested-scope
and top-level decoys → round 5 macro invocation and whitespace variants → round
6 receiver-respelling and indirection family. Every round's fix closed a
textual class that the next round re-expressed through text the new anchors
cannot see; this round found nine compiling survivors at once, all inside the
pin's named fault. The receiver-expression class is unbounded — parens, casts,
derefs, blocks, aliases, UFCS paths, the read-family method names, helper
indirection — and no finite set of string anchors covers it. Each additional
round of string checks buys one more spelling of the same hazard; that is the
definition of diminishing returns, and it is why the reviewer's own records
named the token-level end-state.

**The honest end-state is the token-level pin: parse the body with `syn` and
assert the structure.** Concrete prescription for the remediation cycle (not
implemented here):

1. Add `syn = { version = "2", features = ["full"] }` to `[dev-dependencies]`.
   Zero new compile weight: the lock tree already builds `syn 2.0.119` and
   `3.0.3` (serde_derive/schemars carry the 2.x build).
2. Keep the existing slice (`split("pub fn of_memory")…split("fn of_graph")`)
   and `syn::parse_str::<syn::ItemFn>` it — the slice is exactly one item.
3. Define the graph-guard acquisition structurally. Walk the body for:
   * an `Expr::MethodCall` whose method is in the read family
     {`read`, `read_recursive`, `try_read`, `try_read_recursive`} and whose
     receiver, after unwrapping `ExprParen` / `ExprGroup` / single-expression
     `ExprBlock` / `Expr::Unary(Deref)` / `Expr::Cast`, is either (a) a
     `graph()` method call on a path ending in `memory` (or on a body-level
     `let` alias of `&memory` / `memory` / `&*memory`, collected in one pass
     over the fn's statements — this closes the executed m10 form), or (b) a
     call whose callee path's last segment is `graph` with a single `&memory`
     argument (closes m9 `Memory::graph(&memory)`);
   * an `Expr::Call` whose callee path's last segment is a read-family name
     with the graph lock as its first argument (closes m12
     `parking_lot::RwLock::read(memory.graph())`).
   Parens, whitespace, multiline and turbofish are all gone at token level, so
   m8/m8b, m4/m5/m16, and any generic-method spelling are closed structurally.
4. Assert there is **exactly one** such acquisition in the body; that it sits
   inside the copy block — the `let graph = { … }` statement that is a direct
   statement of `ItemFn.block.stmts` (top-level-ness is free in syn; no depth
   counting) — and that block closes before the `of_graph(…)` statement (the
   guard's binding scope is the block, so "drops before the build" follows).
5. Keep fail-closed on macros as an `Expr::Macro` walk (expansion is invisible
   to the AST too) — this is *more* precise than the text `!(` window: a unary
   not parses as `Expr::Unary`, so the m17 false positive dies with it.
6. Rewrite the doc to claim exactly what the AST checks prove, and to name the
   one class beyond even a token pin: indirection that moves the acquisition
   **out of `of_memory`'s body entirely** (the executed m18 helper; a guard
   returned by a call). That is a structural change to *where* the guard is
   taken, not a respelling of the same code, and it is where the R2-2
   precedent — a documented limit outside the pin's named fault — genuinely
   applies. The syn pin is the honest end-state this sequence named; the helper
   class is the honest boundary past it.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.34 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  530, `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs`
  169, `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs`
  119, `cli/tests.rs` 811, `PLAN.md` 735 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`).
* The seven milestone pins green on the final tree, individually (listed under
  What held).

## What was executed vs. only read

**Executed.** Twenty pin runs — the shipped form twice (baseline and after the
final revert) and eighteen mutations: m1–m18 as tabled, each applied to a byte
copy-restored `view.rs` and `sha256sum`-verified identical (`c30de825…`) after
every run; full panic messages re-captured for the 218:5 and 300:5 and 288:5
forms (verbatim vs the round-5 record); warning counts for all twenty runs; the
m11 compile failure captured (`E0107`); the `!` count of the shipped body slice
(0) and the `self`-receiver question settled (free fn; not applicable); lambo's
`Memory::graph` signature confirmed from the pinned checkout (`&self -> &Arc<
RwLock<Graph>>`, src/memory.rs:1414); the seven milestone pins individually; the
full suite in a clean env; clippy; fmt; the file-size count; the lambo pin
re-confirmed from `Cargo.lock`; the `syn` versions in the lock tree (2.0.119,
3.0.3) for the prescription; the R2-2 acceptance precedent read from
`m12a-round3.md` (lines 212–215) and the M12a `of_graph`-pub standing note from
`m12a-round6.md` (lines 205–208) to ground the "limit outside the named fault"
distinction.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by the mutation
battery; rounds 1–3 already demonstrated the wedged behaviour on this machine
and no lock code changed). The `--demo` pty interplay (round 1 executed it; no
TUI code changed this round). The measurement harness in `view_tick_tests.rs`
(untouched; no production code changed). `syn`'s parse behaviour on the sliced
body (the remediation cycle implements and runs it).

## Notes for M12c

* M12b-R5-1 and M12b-R5-2 are genuinely fixed at their site — the round-5
  remediation does what the record claims, and the round-5 record's correction
  of the round-4 overclaim stands. The residue is the class beyond: nine
  compiling forms pass with the guard held across the build, all inside the
  pin's named fault. Verdict REMEDIATE per the round's instruction; no
  deferrals.
* Implement the token-level pin per the convergence prescription. It closes, in
  one move: the parens-receiver frontier (m8/m8b), the UFCS path spellings
  (m9/m12), the binding alias (m10), the read-family method names (m13),
  deref/block/cast receivers (m14/m15), and the whitespace/multiline class
  (already closed textually, closed again for free at token level); it
  preserves fail-closed-on-macros more precisely (`Expr::Macro`), which also
  kills the m17 `!(…)` false positive. The m18 helper-indirection class is
  beyond even the token pin and must be named in the pin's doc with the R2-2
  argument — a limit outside the named fault (the pin pins acquisitions in
  `of_memory`'s body; a helper moves the acquisition out of the body).
* The milestone itself is unchanged and sound: `view.rs` byte-identical to the
  round-1/round-2/round-3/round-4/round-5 hash, `of_memory` holds the guard for
  exactly one copy, and every pin and gate is green. Every finding in this
  sequence has been about the review pin's coverage, not the code.
* The check line numbers are unchanged from the round-5 record: no-macro 218:5,
  block-close 245:10, depth 267:5, containment 288:5, no-acquisition 300:5.
* The tree stays dirty with the implementation + all five remediations + this
  record, exactly as the orchestrator expects; nothing committed.
