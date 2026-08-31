# M12b round-7 remediation

Remediates the single finding in `m12b-round7.md` — P2 M12b-R7-1 (an
acquisition inside the copy block whose guard value is *consumed* rather
than *bound* passed the pin: `std::mem::forget(guard)` — equally
`ManuallyDrop::new(guard)`, `Box::leak(Box::new(guard))` — skips `Drop`, so
the parking_lot read lock is never released and is held across the whole
`of_graph` build and permanently, the starve-the-writer fault, strictly
worse). No deferrals. The fix is the reviewer's prescription made concrete:
**an acquisition counts only when it is the initializer of a local binding**
(`syn::Stmt::Local` whose `init`, after the existing unwrap, *is* the
acquisition), so an acquisition consumed as a call argument —
`std::mem::forget(memory.graph().read())` — fails the count. Per the
orchestrator's decision (Option B), the check does **not** extend into
consumption analysis: a deliberate value-escape of the bound guard
(`let guard = ...; std::mem::forget(guard);`) stays uncounted and is
documented as a limit with the same status as the round-6 out-of-body
indirection limit — sabotage, not a natural refactor, beyond the named
fault (the pin pins the body's shape and the guard's binding, not what an
author does to the value). One confirming review round follows, then M12b
closes. Base and destination: branch `main` at `709e911`; the tree is left
dirty for the orchestrator, nothing committed. All mutations below were
transient: `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after every run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the
same hash every prior round recorded — this remediation changed `view.rs`
not at all). Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so every
`cargo` invocation ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and
`DATABASE_URL`.

## The design choice: the binding-position count, and the boundary drawn honestly

The round-7 reviewer's prescription was to make the count check bind the
guard: "an acquisition only counts when it is the initializer of a local
binding (`Stmt::Local`'s `init`), so an acquisition consumed as a call
argument (`forget`, `ManuallyDrop`, `Box::leak`) or otherwise unbound fails
the count". That is exactly what the pin now does, with one extension the
round-6 `unwrap` already provided (single-expression blocks are peeled, so
`let guard = { memory.graph().read() };` and `let guard = unsafe { ... };`
still count — the init's *value* is the acquisition):

* **The count is position-bound.** `AcquisitionHunter` counts an
  acquisition only when `is_acquisition(unwrap(&init.expr))` holds for a
  `Stmt::Local`'s initializer. An acquisition that appears anywhere else —
  as a call argument, a bare statement, a scrutinee, a receiver — is not
  the guard and is invisible to the count. Executed: the unbound form
  `std::mem::forget(memory.graph().read());` inside the block (v2) fails
  the count with `left: 0, right: 1` at 331:5.
* **The value-escape of the bound guard is a documented limit, not a
  check.** `let guard = memory.graph().read(); ...; std::mem::forget(guard);`
  still shows the count exactly one (the binding exists), so the pin
  passes it. The round-7 record's executed form is now a pass *by design*:
  a `forget`/`ManuallyDrop`/`Box::leak` of the already-bound guard is a
  deliberate act an author makes on purpose — the record's own words,
  "sabotage, not a natural refactor" — not a respelling or a placement
  change of the shipped shape. It sits outside the named fault exactly as
  the out-of-body indirection helper does, so the R2-2 precedent for a
  documented limit applies to both, and the pin's doc now names both
  boundaries in one place.
* **`drop(guard)` passes** — it is not a leak call and the pin never
  looked for one; the count sees the binding, the guard is released
  immediately, zero warnings (v3).

The rule, stated precisely (and now the pin's doc): the body slice is
parsed as one `syn::ItemFn`; (1) no `Expr::Macro` anywhere in the fn; (2)
the copy is the unique top-level `let graph = { … }` statement, the
whitelisted `let stats = memory.stats();` precedes it, and a top-level
`of_graph(…)` statement follows it; (3) no expression outside the copy's
block references `memory` (confinement); (4) exactly one graph-guard
acquisition exists, inside the copy's block, **and it is the initializer
of a local binding** — so the guard the count sees is a bound local whose
binding scope is the block. What the count cannot see, and the doc says
so: an acquisition consumed as a call argument fails the count; an
acquisition moved **out of** the body (a helper returning the guard) and a
**deliberate value-escape of the bound guard** (`std::mem::forget(guard)`,
`Box::leak(Box::new(guard))`, `ManuallyDrop::new(guard)` after the binding)
are the two documented limits, both R2-2.

Two small collateral adjustments, both keeping the historical battery
intact:

* `unwrap` now also peels a single-expression `unsafe` block, so the
  round-7 safe form (u1) `let guard = unsafe { (memory.graph()).read() };`
  still counts (the init's value is the acquisition; the guard is bound and
  drops at the close). The `unnecessary unsafe` warning is the compiler's.
* Check 2's doc and check 4's doc were rewritten to claim exactly what the
  checks prove — the guard is a bound local whose binding scope is the
  block — and to name the value-escape limit; the false claim the round-7
  finding quoted ("Exactly one, inside the block, is the guard that drops
  at the block's close") is gone.

## M12b-R7-1 — the count now binds the guard

The round-7 executed mutation, verbatim on the shipped body:

```rust
    let graph = {
        let guard = memory.graph().read();
        let data = ViewData::from_graph(&guard);
        std::mem::forget(guard);
        data
    };
```

The count still sees exactly one acquisition (the `let guard = ...`
binding), so check 4's number passes — and under Option B that is the
documented limit, not a failure: the pin proves the guard is a bound
local, and a `forget` of that binding is sabotage, beyond the named fault
(the same status the round-6 review gave the out-of-body helper). What
*fails* now is the class the reviewer's prescription aimed at: an
acquisition **consumed as a call argument**, never bound — (v2)
`std::mem::forget(memory.graph().read());` inside the block (a guard
forgotten the moment it is taken, the same permanent starve) fails the
count at 331:5 with `left: 0, right: 1`. The pin's doc claim is now
literally true for every form it passes: the guard is a bound local, and
the binding's scope is the block.

## The proof

Every mutation transient; `src/memory/view.rs` restored from a byte copy
and `sha256sum`-verified identical (`c30de825…`) after each. The pin run
on the shipped bytes before the battery and again as the battery's (a)
row and after the final revert. All runs in a clean env (the three DSN
variables unset). 45 battery runs + the standalone shipped baselines = 48
executed pin runs, 45/45 expected outcomes, 0 mismatches. One format note:
this toolchain's rustc renders a panic message on the line after
`panicked at <loc>:`; the records' inline rendering is the same bytes with
the line break at the colon. The check line numbers moved with the doc
rework: parse 241:85, macro 253:5, copy expect 263:61, uniqueness 268:5,
stats expect 275:57, stats order 279:5, build expect 284:57, confinement
308:9, count 331:5 (round-7 record's 227:85…312:5 quotes are historical).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — three times: the standalone baseline before the battery, the battery's (a) row, and the final re-run on the restored bytes; `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out`, zero warnings |
| **(v1) the round-7 form: `let guard = ...; let data = ViewData::from_graph(&guard); std::mem::forget(guard); data` inside the block** | same | **passes** — `ok. 1 passed`, zero warnings: the count sees the one binding; the value-escape of the bound guard is the documented limit (Option B), the round-7 finding's executed form is now a pass by design |
| **(v1m) `let _ = std::mem::ManuallyDrop::new(guard);` variant** | same | **passes** — `ok. 1 passed`, zero warnings (same documented limit) |
| **(v1b) `let _ = Box::leak(Box::new(guard));` variant** | same | **passes** — `ok. 1 passed`, zero warnings (same documented limit) |
| **(v2) unbound: `std::mem::forget(memory.graph().read());` inside the block (a second unbound acquisition in the data line)** | same | **caught** — `panicked at src/memory/view_session_tests.rs:331:5:` `assertion `left == right` failed: exactly one graph-guard acquisition may appear in of_memory's body: a read-family method call (`read`, `read_recursive`, `try_read`, `try_read_recursive`) on the memory graph's lock, or a read-family call taking the graph as its first argument, however the receiver is spelled`, left: 0, right: 1, zero warnings — an acquisition consumed as a call argument is never the guard |
| **(v3) `drop(guard);` inside the block after the copy** | same | **passes** — `ok. 1 passed`, zero warnings — `drop` is a release, not an escape: no false positive |
| (m8) `let guard = (memory.graph()).read();` after the block | same | **caught** — `panicked at src/memory/view_session_tests.rs:308:9:` `no expression outside the copy's block may reference the `memory` parameter: the guard is taken only inside the block, so a memory reference after its close is an acquisition (or the receiver, alias or argument of one) bound at function scope and held across the build`; one `unused variable: guard` warning |
| (m9) `Memory::graph(&memory).read()` after the block (UFCS on the `&self` method) | same | **caught** — same 308:9 message; one warning |
| (m10) `let g = &memory; let guard = g.graph().read();` after the block (binding alias) | same | **caught** — same 308:9 message; one warning (the `&memory` binding outside the block is itself the reference) |
| (m12) `parking_lot::RwLock::read(memory.graph())` after the block (UFCS on the lock) | same | **caught** — same 308:9 message; one warning |
| (m13) `memory.graph().try_read().unwrap()` after the block (read-family name) | same | **caught** — same 308:9 message; one warning |
| (m14) `(*memory).graph().read()` after the block (deref receiver) | same | **caught** — same 308:9 message; one warning |
| (m15) `{ memory }.graph().read()` after the block (block receiver) | same | **caught** — same 308:9 message; one warning |
| (m8b) `(memory.graph()).read();` hoisted above the block | same | **caught** — same 308:9 message; one warning |
| (m18) module `fn read_lock(m: &Memory)` + `let guard = read_lock(&memory);` after the block | same | **caught** — same 308:9 message (the `&memory` argument); one warning |
| (b) flat three-statement form (round-1 class) | same | **caught** — `panicked at src/memory/view_session_tests.rs:263:61:` `the copy is a block: of_memory must copy the graph out from under the guard inside a `let graph = { … }` statement, so the guard's binding scope ends at the block's close`; zero warnings |
| (c) guard hoisted above the block, block retained | same | **caught** — same 308:9 message; one warning |
| (d) decoy `let graph = { 0 };` before the copy | same | **caught** — `panicked at src/memory/view_session_tests.rs:268:5:` `assertion `left == right` failed: exactly one `let graph = { … }` statement may appear at of_memory's top level: the copy is anchored by structure, so a decoy block cannot be the block the guard drops inside`, left: 2, right: 1; one warning |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — same 308:9 message, one warning (comments never reach the AST) |
| (h1) closure carrying a complete guard+copy before a hoisted guard | same | **caught** — same 308:9 message, one warning (the walk descends into the closure body) |
| (n1) module `macro_rules! grab` + `let guard = grab!(memory);` after the block | same | **caught** — `panicked at src/memory/view_session_tests.rs:253:5:` `no macro invocation may appear in of_memory's body: an invocation's expansion is invisible to this AST pin, so it could acquire the graph guard at function scope and hold it across the build`; one warning (M12b-R5-1 stays closed) |
| (o2) `let inner = grab!(memory);` inside the copy block | same | **caught** — same 253:5 message; one `unused variable: inner` warning |
| (n2) spaced acquisition `memory.graph().read ();` after the close | same | **caught** — same 308:9 message; one warning (M12b-R5-2 stays closed) |
| (o3) `memory\n.graph()\n.read();` after the close | same | **caught** — same 308:9 message; one warning |
| (o4) `let guard = unsafe { memory.graph().read() };` after the close | same | **caught** — same 308:9 message; two warnings (`unnecessary \`unsafe\` block`, `unused variable: guard`) |
| (iv-iflet) guard acquired in an `if let` scrutinee after the close | same | **caught** — same 308:9 message, zero warnings |
| (p1) `let guard = read_lock(&memory);` **inside** the copy block (module helper; safe shape, unverifiable) | same | **caught** — `panicked at src/memory/view_session_tests.rs:331:5:` `assertion `left == right` failed: exactly one graph-guard acquisition may appear in of_memory's body: …`, left: 0, right: 1; zero warnings (fail-closed on helper expansion) |
| (p2) two acquisitions inside the copy block (`g1`, `g2` both `memory.graph().read()`) | same | **caught** — same 331:5 message, left: 2, right: 1; zero warnings |
| (p3) `let stats = memory.stats();` moved after the copy | same | **caught** — `panicked at src/memory/view_session_tests.rs:279:5:` `the figures must be read before the copy: `Memory::stats` takes the graph lock itself, and the read lock is not recursion-safe`; zero warnings |
| (p4) build folded into the copy block (block value returned by a tail `graph`) | same | **caught** — `panicked at src/memory/view_session_tests.rs:284:57:` `the build follows the copy: the body must end with a top-level `of_graph(…)` statement`; zero warnings |
| (w1) `let stats = (&memory).stats();` replacing the real figures statement | same | **caught** — `panicked at src/memory/view_session_tests.rs:275:57:` `the figures are read first: a `let stats = memory.stats();` statement must precede the copy`; zero warnings (the whitelist matches only the exact path receiver — fail-closed) |
| (w2) extra pre-block `let lookalike = memory.graph().read();` beside the real stats | same | **caught** — same 308:9 message; one warning |
| (s1) `// fn of_graph` comment inside the body (slice cut mid-body) | same | **caught** — `panicked at src/memory/view_session_tests.rs:241:85:` `of_memory's body slice parses as a function item: the pin judges the AST, and cannot judge source it cannot parse: Error("cannot parse string into token stream")` — fail-loud |
| (s3) `let _s = "fn of_graph";` string inside the body (slice cut inside a literal) | same | **caught** — same 241:85 parse-expect panic — fail-loud, fail-closed |
| (s2) of_graph's doc comment gains a `/// fn of_graph` line | same | **passes** — `ok. 1 passed`, zero warnings; `body_close` truncates at of_memory's own close before the doc comment |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok. 1 passed`, zero warnings; no false positive |
| (i1) safe nested guard scope: copy wrapped in one extra `{ }` layer | same | **passes** — `ok. 1 passed`, zero warnings; the acquisition still sits inside the copy's block |
| (l) safe extra braces: `let guard = { memory.graph().read() };` inside the block | same | **passes** — `ok. 1 passed`, zero warnings (unwrap peels the single-expression block, so the init's value is the acquisition) |
| (m1) safe `#[allow(unused_variables)]` attribute on the copy statement | same | **passes** — `ok. 1 passed`, zero warnings |
| (v-const) safe `const _: usize = { 1 + 1 };` between the body brace and the copy | same | **passes** — `ok. 1 passed`, one `unnecessary braces around assigned value` warning on the mutation's own `{ 1 + 1 }` |
| (m17) `let _b = !(1 > 2);` inside the body (legitimate unary not) | same | **passes** — `ok. 1 passed`, zero warnings (P3 M12b-R6-2 stays closed: `Expr::Unary`, never `Expr::Macro`) |
| (u1) safe `let guard = unsafe { (memory.graph()).read() };` inside the block | same | **passes** — `ok. 1 passed`, one `unnecessary \`unsafe\` block` warning (unwrap now peels a single-expression unsafe block, so the init's value is the acquisition; the guard still drops at the close) |
| (a1) safe chained aliases: `let m = memory; let g = &m; let guard = g.graph().read();` inside the block | same | **passes** — `ok. 1 passed`, zero warnings (the one-pass alias map resolves the chain; count exactly one) |
| (a3) `let memory = 0;` shadowing after the block | same | **passes** — `ok. 1 passed`, one `unused variable: memory` warning; harmless (a shadowed non-memory value cannot acquire the graph lock) |
| (g1) module `fn global_guard()` reading a `static` `OnceLock`-held graph + `let guard = global_guard();` after the block (no memory argument) | same | **passes** — `ok. 1 passed`, one `unused variable: guard` warning — the documented out-of-body limit, exactly as named, R2-2 precedent applies |

Each mutation reverted and hash-verified identical (`c30de825…`) after the
run; the shipped-form runs came from the restored bytes. The checks fail in
order: parse 241:85, macro 253:5, copy expect 263:61, uniqueness 268:5,
stats expect 275:57, stats order 279:5, build expect 284:57, confinement
308:9, count 331:5. All nine round-6 forms fail at confinement; every
historical evasion fails at its check; the unbound consumption class fails
the count; `drop(guard)` and every safe form pass; the bound-then-forgotten
forms (v1, v1m, v1b) pass as the documented limit, exactly as the
orchestrator's Option-B decision prescribes.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset), on the final tree:

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.31 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  795 (was 760; +35 — the count reworked to the binding-position rule, the
  `unwrap` unsafe-block peel, and the doc rewritten to claim the bound-local
  proof and name the value-escape limit), `view_tests.rs` 871,
  `view_clock_tests.rs` 292, `view_tick_tests.rs` 169, `tui/mod.rs` 807,
  `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs` 119, `cli/tests.rs`
  811, `PLAN.md` 735, `Cargo.toml` 62 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`);
  no dependency change this round.
* The seven milestone pins green on the final tree, individually: the three
  M12b pins (`a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
  `a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
  `the_figures_are_read_before_the_graph_guard`) and the four M12a
  regression pins (`the_local_database_is_created_and_repaired_private`,
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`); the
  pin under remediation ran green on the shipped bytes before the battery
  and again after the final revert.
* `src/memory/view.rs` byte-identical to the recorded hash (`c30de825…`) at
  the end; `git status --porcelain` shows exactly the pre-review set (12
  modified + 13 untracked) plus this record; nothing committed.

## What was executed vs. only read

**Executed.** Forty-eight pin runs: the shipped form three times (standalone
baseline before the battery, the battery's (a) row, and the final re-run on
the restored bytes) and forty-five mutations as tabled — the round-7
value-destination family (v1/v1m/v1b/v3/v2), the nine round-6 forms, the
historical evasions, the safe forms, and the count-focus probes (p1, p2,
a1, g1) — every one applied to a byte copy-restored `view.rs` and
`sha256sum`-verified identical (`c30de825…`) before and after every run
(the harness asserted the hash after each of the 45 battery runs); full
panic messages re-captured with line and column for the representative
forms (241:85, 253:5, 263:61, 268:5, 275:57, 279:5, 284:57, 308:9, 331:5);
warning counts for every run; the compile of every mutation verified (each
mutation must compile — the whole test target builds); the syn 2.0.119
visitor API confirmed from the vendored source (`visit_stmt` dispatches
`Stmt::Local` through `visit_local`, which visits `local.init` via
`visit_local_init`; `LocalInit { eq_token, expr, diverge }`); lambo's
`Graph::new(SessionId)` and `lambo::types::SessionId` confirmed from the
pinned checkout for the (g1) helper mutation; the seven milestone pins; the
full suite in a clean env; clippy; fmt; the file-size count.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by the battery;
rounds 1–3 already demonstrated the wedged behaviour on this machine and no
lock code changed). The `--demo` pty interplay (round 1 executed it; this
remediation touches no TUI code). The measurement harness in
`view_tick_tests.rs` (untouched; no production code changed).

## Notes for M12c

* M12b-R7-1 is closed at its site by the reviewer's prescription: the count
  now proves the guard is a **bound local** — an acquisition counts only as
  the initializer of a `Stmt::Local` (after the existing unwrap), so an
  acquisition consumed as a call argument (`std::mem::forget(memory.graph().
  read())`, equally `Box::leak`, `ManuallyDrop`) is invisible to the count
  and fails it (executed v2: `left: 0, right: 1` at 331:5). The round-7
  finding's executed form — `let guard = ...; std::mem::forget(guard);` —
  passes **by design**: the orchestrator's Option-B decision makes the
  value-escape of the bound guard a documented limit, not a check.
* The boundary is named in the pin's doc with the same status as the
  round-6 limit. Two documented limits now sit beyond the named fault, both
  R2-2: an acquisition moved **out of** [`of_memory`]'s body entirely (a
  helper that returns the guard, or a caller that takes the copy with it),
  and a **deliberate value-escape of the bound guard** (`std::mem::forget`,
  `Box::leak(Box::new(…))`, `ManuallyDrop::new` after the binding) — the
  second is sabotage, not a natural refactor of the shipped shape, and the
  pin pins the body's shape and the guard's binding, not what an author
  does to the value. `drop(guard)` is not in the escape class — it releases
  the lock — and passes (v3, zero warnings).
* The doc claims were corrected to match the checks: check 4 proves the
  guard is a bound local whose binding scope is the block (the round-7
  finding quoted the old "is the guard that drops at the block's close"
  claim as false for the forget form; that claim is gone, and the value
  that is not dropped is the documented limit). Check 2's doc carries the
  same caveat.
* One collateral extension, keeping the historical battery intact: `unwrap`
  now peels a single-expression `unsafe` block, so the round-7 safe form
  (u1) `let guard = unsafe { (memory.graph()).read() };` still counts and
  passes (the init's value is the acquisition; the `unnecessary unsafe`
  warning is the compiler's).
* The check line numbers moved with the doc rework: parse 241:85, macro
  253:5, copy expect 263:61, uniqueness 268:5, stats expect 275:57, stats
  order 279:5, build expect 284:57, confinement 308:9, count 331:5. The
  round-6 record's 227:85…312:5 quotes are historical.
* The milestone itself is unchanged and sound: `view.rs` byte-identical to
  the round-1-through-round-7 hash (`c30de825…`, 975 lines), `of_memory`
  holds the guard for exactly one copy, and every pin and gate is green.
  Seven rounds of findings have been about the review pin's coverage, not
  the code; the coverage question now has its two named boundaries and no
  unnamed survivor.
* The tree stays dirty with the implementation + all seven remediations +
  Cargo.toml/Cargo.lock + this record, exactly as the orchestrator expects;
  nothing committed. One confirming review round follows per the
  orchestrator's decision, then M12b closes.
