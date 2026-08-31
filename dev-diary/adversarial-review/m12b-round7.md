# M12b round 7 — adversarial re-review of the round-6 remediation (the syn pin)

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1 through round-6 remediations (12 modified + 13 untracked —
the full expected set, verified identical before and after every mutation, now
with this record beside them). Scope: the structural syn pin
(`the_build_runs_against_the_copy_and_not_the_guard` in
`src/memory/view_session_tests.rs`, now parsing `of_memory`'s body with `syn`
and asserting shape: no `Expr::Macro`, one top-level `let graph = { … }`, the
whitelisted pre-block `let stats = memory.stats();`, a top-level
`of_graph(…)` after the copy, **confinement** — no expression outside the
copy's block references `memory` — and exactly one graph-guard acquisition,
inside the block), the two round-6 findings it closes (P2 M12b-R6-1, the nine
receiver-respelling/indirection forms; P3 M12b-R6-2, the `!(…)` false
positive), the alias-map/closure/whitelist/slice/parse-failure attack surface,
and the honesty of the documented limit (indirection moving the acquisition
out of `of_memory`'s body entirely). All 45 transient mutations reverted and
`sha256sum`-verified byte-identical to the pre-mutation state after every run
(`src/memory/view.rs` `c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`,
the same hash every prior round recorded — the round-6 remediation changed
`view.rs` not at all, and neither did this round); `git status --porcelain`
shows exactly the same 12 modified + 13 untracked as before I started (plus
this record), nothing committed. Ambient shell exports a live
`LAMBO_POSTGRES_DSN`, so every `cargo` invocation ran under `env -u` for it,
`MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 1 × P2.

The round-6 remediation is genuine and the structural pin has converged on
the class it was built for: all nine round-6 forms, every historical evasion,
and this round's new attacks (chained aliases, shadowing, dead closures, the
whitelist's narrowness, the slice's robustness, unsafe-in-block, the
documented-limit form) behave exactly as the remediation record claims — 45
executed forms, 45 expected outcomes, 0 mismatches. The confinement rule is
sound for every memory-derived guard: an acquisition outside the block is
impossible without a `memory` reference outside the block (the alias of a
memory-derived value cannot exist outside the block either — its binding
references `memory` and is caught), and the whitelisted `let stats =
memory.stats();` is exactly as narrow as documented. What survives is a
single value-escape class the pin's checks do not see and the doc does not
name: **an acquisition inside the copy block whose guard value is consumed
rather than bound** — `std::mem::forget(guard)` (equally `ManuallyDrop`,
`Box::leak`) leaks the read lock: it is held across the whole build and
permanently, which is the named writer-starvation hazard, strictly worse.
Executed: mutation (v1) passes the pin with `ok. 1 passed` and zero warnings.
Check 4's own doc claim — "Exactly one, inside the block, is the guard that
drops at the block's close" — is false for that form; no check verifies that
the acquisition's value is bound by a `let` whose scope is the block. Per the
standing convention (a pin passing with the hazard present is a P2, no
deferrals), that is M12b-R7-1 below. The documented out-of-body limit is
honest and acceptable (executed: the no-argument helper reading a static
graph passes as documented — it is the named "helper that returns the guard"
class, and the R2-2 precedent applies); its one imprecise clause is noted,
not found.

## Findings

### P2

**M12b-R7-1 — the pin passes when the in-block acquisition's guard is leaked
(`std::mem::forget`), holding the graph lock across the build and forever.**

The count checks (4) prove "exactly one graph-guard acquisition exists,
inside the copy's block" — they never prove what happens to the guard value.
Executed mutation (v1) on the shipped body:

```rust
    let graph = {
        let guard = memory.graph().read();
        let data = ViewData::from_graph(&guard);
        std::mem::forget(guard);
        data
    };
```

compiles cleanly and the pin passes: `test result: ok. 1 passed; 0 failed;
0 ignored; 0 measured; 545 filtered out` with **zero warnings**. The count
sees the one acquisition (`memory.graph().read()`), confinement is satisfied
(the statement sits inside the block), and nothing checks that the guard is
*bound* — `mem::forget` consumes it, so `Drop` never runs, the read-lock slot
is never released, and every writer to the memory graph blocks from the
moment the copy's block closes, across `of_graph` and permanently. That is
the writer-starvation fault the copy exists to prevent, strictly worse (the
build-span hold was the named fault; a forgotten guard is a permanent hold).
Check 4's doc claim — "Exactly one, inside the block, is the guard that drops
at the block's close" — is false for this form, and the value-escape class is
not the documented limit (the doc names only indirection that moves the
acquisition **out of** the body — "a helper that returns the guard, or a
caller that takes the copy with it"; here the acquisition is in the body).
`ManuallyDrop::new(guard)` and `Box::leak(Box::new(guard))` are the same
class. Every other executed form — including the direct spellings of all nine
round-6 forms, the alias chains, closures, macros, and the in-block helper
call (p1) — fails closed; this is the single survivor in 45 runs.

*Remediation sketch.* Make the count check bind the guard: an acquisition
only counts when it is the initializer of a local binding (`Stmt::Local`'s
`init`), so an acquisition consumed as a call argument (`forget`,
`ManuallyDrop`, `Box::leak`) or otherwise unbound fails the count — a small
change to `AcquisitionHunter` (skip non-`LocalInit` parents), which then
makes the doc's "the guard's binding scope is the block" literally true. The
alternative — extending the doc's limit to name the value-escape class with
the R2-2 argument — is weaker (the effect is a *permanent* starve, inside the
body, so it does not sit as cleanly outside the named fault as the out-of-body
helper does). Either closes the class; the binding check is the honest one.

## What held up under attack

* **The nine round-6 forms fail at confinement, verbatim class.** (m8)
  `(memory.graph()).read()`, (m9) `Memory::graph(&memory).read()`, (m10)
  `let g = &memory; g.graph().read()`, (m12)
  `parking_lot::RwLock::read(memory.graph())`, (m13) `try_read().unwrap()`,
  (m14) `(*memory).graph().read()`, (m15) `{ memory }.graph().read()`, (m18)
  `read_lock(&memory)` via a module-level helper, and the hoisted (m8b) — all
  nine **caught at 294:9** ("no expression outside the copy's block may
  reference the `memory` parameter"), each with the `unused variable: guard`
  warning. The record's claims re-verified exactly.
* **The historical battery behaves.** (b) flat → **249:61** (copy expect);
  (c) hoisted direct → **294:9**; (d) decoy `let graph = { 0 };` → **254:5**
  (uniqueness, left: 2, right: 1); (e) comment anchor → **294:9** (comments
  never reach the AST); (h1) closure carrying guard+copy → **294:9** (the
  walk descends into the closure body); (n1) module `macro_rules!` +
  `grab!(memory)` after the close → **239:5** (no-macro); (o2) `grab!(memory)`
  inside the block → **239:5**; (n2) `read ();` spaced and (o3) multiline →
  **294:9**; (o4) `unsafe { … }` after the close → **294:9** (+
  `unnecessary unsafe`); (p1) `read_lock(&memory)` inside the block →
  **312:5** (count, left: 0, right: 1 — fail-closed on helper expansion);
  (p2) two acquisitions in the block → **312:5** (left: 2); (p3) stats after
  copy → **265:5**; (p4) build folded into the block → **270:57**. All anchors
  match the round-6 record's line numbers.
* **The safe forms pass.** (f4) string `{` inside the block, (i1) copy wrapped
  in an extra `{ }` layer, (l) guard RHS wrapped in braces, (m1)
  `#[allow(unused_variables)]` on the copy, (v-const) a `const` between the
  brace and the copy, (m17) `let _b = !(1 > 2);` — all **pass**, m17 with zero
  warnings (P3 M12b-R6-2 closed: `Expr::Unary`, never `Expr::Macro`).
* **Alias-map completeness.** (a1) `let m = memory; let g = &m;
  g.graph().read()` inside the block → **passes** (the chain resolves: the
  one-pass map records `m → memory`, `g → m`, and the recursion in
  `alias_resolves_to_memory` follows it; the count is exactly one — the
  safe-direction check holds); (a4) `let g = &*memory;` re-borrow → **passes**
  (unwrap peels Reference→Deref). (a2) the same chain bound *after* the block
  → **caught at 294:9** (the `let m = memory;` binding is itself the memory
  reference). (a3) `let memory = 0;` shadowing after the block → **passes**,
  harmlessly: a shadowed non-memory value cannot acquire the graph lock, and
  any shadowing with a memory-derived value must reference the parameter (or
  an alias of it) and is caught at its binding. The map's scope-blindness errs
  fail-closed (an out-of-scope name still resolves and over-counts).
* **Closures.** (b1) a dead `|| memory.stats()` closure and (b2) a dead
  closure acquiring a guard, both after the block → **caught at 294:9**: the
  confinement visitor descends into `Expr::Closure` bodies, and deadness is
  irrelevant — the tokens are judged, not the execution.
* **The whitelist is exactly as narrow as documented.** (w1) `let stats =
  (&memory).stats();` replacing the real statement → **caught at 261:57**
  (the stats expect — the whitelist matches only `Expr::Path` receiver
  `memory`, so a respelled receiver fails closed, it cannot slip); (w2) an
  extra pre-block `memory.graph().read()` beside the real stats → **caught at
  294:9**. No refactor can make the whitelisted pre-block reference an
  acquiring call: either the statement stops matching `is_stats_stmt` (expect
  panics) or it is an additional memory reference (confinement fires); and
  `memory.stats()` itself is never an acquisition (`stats` ∉ the read family).
* **The slice parses exactly as one item, and fails loud, never silent.**
  `body_close` truncates at the fn's own body brace (comment- and
  literal-aware) before parsing, so of_graph's doc comment — including a line
  that itself spells `fn of_graph` (s2, executed) — never affects the parse:
  (s2) → **passes**. When the split string appears *inside* of_memory's body
  — a comment (s1) or a string literal (s3) containing `fn of_graph` — the
  slice cuts early and the truncated text no longer parses; both **panic at
  227:85** ("of_memory's body slice parses as a function item: the pin judges
  the AST, and cannot judge source it cannot parse: Error(\"cannot parse
  string into token stream\")"). Fail-loud, fail-closed. (The s1 cut lands
  mid-body, so `body_close` never finds the close and the unterminated slice
  is what fails the parse.) No silent skip exists: an unparseable slice is
  the parse expect; an unparseable *file* fails the build.
* **Unsafe inside the block is allowed.** (u1) `let guard = unsafe {
  (memory.graph()).read() };` inside the block → **passes** (the acquisition
  is counted through the unsafe block; the guard still drops at the close —
  the `unnecessary unsafe` warning is the compiler's, not the pin's).
* **Bound-then-dropped still counts.** (v3) `drop(guard);` after the copy →
  **passes** (the acquisition expression is visited regardless of what the
  binding does next; the guard drops immediately — safe).
* **The documented limit is real and honest.** (g1) a module-level
  `fn global_guard() -> parking_lot::RwLockReadGuard<'static, Graph>` reading
  a `static` `OnceLock`-held graph, called as `let guard = global_guard();`
  after the block (no memory argument) → **passes**, as documented: the
  acquisition is in the helper's body, the body's count stays one, and
  confinement has nothing to see. This is precisely the named limit — "a
  helper that returns the guard" — and the R2-2 precedent (a documented limit
  outside the pin's named fault) applies, exactly as the round-6 record and
  the pin's doc say. The count check even catches the *replacement* variant
  (guard-only helper, no in-block acquisition → found 0 → 312:5), which is
  stronger than the doc promises. The one imprecise clause — the doc's "the
  body then contains no acquisition tokens at all" — is false in the additive
  variant (real acquisition retained + helper call), but the class ("a helper
  that returns the guard") is named without qualification and the boundary is
  drawn correctly; a wording note for the remediator, not a finding.
* **The milestone still holds.** `view.rs` byte-identical to
  `c30de825…` (975 lines) before, throughout and after the battery;
  `of_memory` holds the guard for exactly one copy; all seven pins green,
  individually: the three M12b pins (`a_rebuild_sees_a_write_from_elsewhere_
  without_a_keystroke`, `a_tick_rebuilds_the_live_workspace_and_leaves_the_
  demo_alone`, `the_figures_are_read_before_the_graph_guard`) and the four
  M12a regression pins (`the_local_database_is_created_and_repaired_private`,
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`) — each
  `test result: ok. 1 passed; 0 failed; 545 filtered out`.
* **Gates.** Full suite in a clean env, clippy, fmt, file-size caps, lambo
  pin, Cargo.lock — all as recorded below.

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). All runs under a clean env (the three DSN variables unset). 44
mutations + 2 shipped-form runs (baseline before the battery and the final
milestone re-run) = 46 pin runs, 45/45 expected outcomes, 0 mismatches. The
pin: `the_build_runs_against_the_copy_and_not_the_guard`.

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | same | **passes** — baseline before the battery, `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out`; re-run green after the battery |
| (m8) `let guard = (memory.graph()).read();` after the block — the documented parens frontier | same | **caught** — `panicked at src/memory/view_session_tests.rs:294:9: no expression outside the copy's block may reference the `memory` parameter: …`; one `unused variable: guard` warning |
| (m9) `Memory::graph(&memory).read()` after the block (UFCS method) | same | **caught** — same 294:9 message; one warning |
| (m10) `let g = &memory; let guard = g.graph().read();` after the block (binding alias) | same | **caught** — same 294:9 message; one warning |
| (m12) `parking_lot::RwLock::read(memory.graph())` after the block (UFCS lock) | same | **caught** — same 294:9 message; one warning |
| (m13) `memory.graph().try_read().unwrap()` after the block (read-family name) | same | **caught** — same 294:9 message; one warning |
| (m14) `(*memory).graph().read()` after the block (deref receiver) | same | **caught** — same 294:9 message; one warning |
| (m15) `{ memory }.graph().read()` after the block (block receiver) | same | **caught** — same 294:9 message; one warning |
| (m8b) `(memory.graph()).read()` hoisted above the block | same | **caught** — same 294:9 message, zero warnings |
| (m18) module `fn read_lock(m: &Memory) -> parking_lot::RwLockReadGuard<'_, Graph>` + `let guard = read_lock(&memory);` after the block | same | **caught** — same 294:9 message (the `&memory` argument); one warning |
| (b) flat three-statement form (round-1 class) | same | **caught** — `panicked at …view_session_tests.rs:249:61: the copy is a block: …`; zero warnings |
| (c) guard hoisted above the block, block retained | same | **caught** — same 294:9 message; one warning |
| (d) decoy `let graph = { 0 };` before a hoisted guard | same | **caught** — `254:5: assertion `left == right` failed: exactly one `let graph = { … }` statement …`, left: 2, right: 1; one warning |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — same 294:9 message, zero warnings (comments never reach the AST) |
| (h1) closure carrying a complete guard+copy before a hoisted guard | same | **caught** — same 294:9 message, zero warnings (the walk descends into the closure body) |
| (n1) module `macro_rules! grab` + `let guard = grab!(memory);` after the block | same | **caught** — `panicked at …view_session_tests.rs:239:5: no macro invocation may appear in of_memory's body: …`; one warning (M12b-R5-1 stays closed) |
| (o2) `let inner = grab!(memory);` inside the copy block | same | **caught** — same 239:5 message, zero warnings |
| (n2) spaced `memory.graph().read ();` after the close | same | **caught** — same 294:9 message; one warning (M12b-R5-2 stays closed) |
| (o3) `memory\n.graph()\n.read();` after the close | same | **caught** — same 294:9 message; one warning |
| (o4) `let guard = unsafe { memory.graph().read() };` after the close | same | **caught** — same 294:9 message; two warnings (`unnecessary \`unsafe\` block`, `unused variable: guard`) |
| (p1) `let guard = read_lock(&memory);` inside the copy block (module helper; safe shape, unverifiable) | same | **caught** — `312:5: assertion `left == right` failed: exactly one graph-guard acquisition may appear in of_memory's body: …`, left: 0, right: 1; zero warnings (fail-closed on helper expansion) |
| (p2) two acquisitions inside the copy block (`g1`, `g2` both `memory.graph().read()`) | same | **caught** — same 312:5 message, left: 2, right: 1; one warning |
| (p3) `let stats = memory.stats();` moved after the copy | same | **caught** — `panicked at …view_session_tests.rs:265:5: the figures must be read before the copy: …`; zero warnings |
| (p4) build folded into the copy block (block value returned by a tail `graph`) | same | **caught** — `panicked at …view_session_tests.rs:270:57: the build follows the copy: …`; zero warnings |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok. 1 passed`, zero warnings; no false positive |
| (i1/l) safe nested guard scope: copy wrapped in one extra `{ }` layer | same | **passes** — `ok. 1 passed`, zero warnings |
| (l) safe extra braces: `let guard = { memory.graph().read() };` inside the block | same | **passes** — `ok. 1 passed`, zero warnings (unwrap peels the single-expression block) |
| (m1) safe `#[allow(unused_variables)]` attribute on the copy statement | same | **passes** — `ok. 1 passed`, zero warnings |
| (v-const) safe `const _: usize = { 1 + 1 };` between the body brace and the copy | same | **passes** — `ok. 1 passed`, one `unnecessary braces` warning on the mutation's own `{ 1 + 1 }` |
| **(m17) `let _b = !(1 > 2);` inside the body (legitimate unary not)** | same | **passes** — `ok. 1 passed`, zero warnings (P3 M12b-R6-2 closed: `Expr::Unary`, never `Expr::Macro`) |
| (a1) safe chained aliases: `let m = memory; let g = &m; let guard = g.graph().read();` inside the block | same | **passes** — `ok. 1 passed`, zero warnings (the one-pass alias map resolves the chain; count exactly one) |
| (a2) the same chain bound after the block | same | **caught** — same 294:9 message (the `let m = memory;` binding is itself the reference), zero warnings |
| (a3) `let memory = 0;` shadowing after the block | same | **passes** — `ok. 1 passed`, one `unused variable: memory` warning; harmless (a shadowed non-memory value cannot acquire the graph lock; a memory-derived shadow must reference the parameter and is caught) |
| (a4) safe re-borrow: `let g = &*memory; let guard = g.graph().read();` inside the block | same | **passes** — `ok. 1 passed`, zero warnings |
| (b1) dead `let dead = || memory.stats();` after the block | same | **caught** — same 294:9 message (the confinement walk descends into the closure body; deadness irrelevant), zero warnings |
| (b2) dead closure acquiring inside its body after the block | same | **caught** — same 294:9 message, zero warnings |
| (w1) `let stats = (&memory).stats();` replacing the real figures statement | same | **caught** — `panicked at …view_session_tests.rs:261:57: the figures are read first: a `let stats = memory.stats();` statement must precede the copy` (the whitelist matches only the exact path receiver — fail-closed), zero warnings |
| (w2) extra pre-block `let lookalike = memory.graph().read();` beside the real stats | same | **caught** — same 294:9 message; one warning |
| (s1) `// fn of_graph` comment inside the body (slice cut mid-body) | same | **caught** — `panicked at …view_session_tests.rs:227:85: of_memory's body slice parses as a function item: the pin judges the AST, and cannot judge source it cannot parse: Error("cannot parse string into token stream")` — fail-loud |
| (s2) of_graph's doc comment gains a `/// fn of_graph` line | same | **passes** — `ok. 1 passed`, zero warnings; `body_close` truncates at of_memory's own close before the doc comment, so doc-comment changes cannot break the parse |
| (s3) `let _s = "fn of_graph";` string inside the body (slice cut inside a literal) | same | **caught** — same 227:85 parse-expect panic (unterminated literal consumes the truncated slice; depth never closes) — fail-loud, fail-closed |
| (u1) safe `let guard = unsafe { (memory.graph()).read() };` inside the block | same | **passes** — `ok. 1 passed`, one `unnecessary \`unsafe\` block` warning (the acquisition is counted through the unsafe block; the guard still drops at the close) |
| **(v1) `std::mem::forget(guard);` inside the copy block, after the copy** | same | **PASSES** — `ok. 1 passed`, zero warnings; the guard is consumed, never dropped — the read lock is held across the build and permanently (P2, M12b-R7-1) |
| (v3) safe `drop(guard);` inside the block after the copy | same | **passes** — `ok. 1 passed`, zero warnings (bound-then-dropped still counts; the guard drops immediately) |
| (g1) module `fn global_guard()` reading a `static` `OnceLock`-held graph + `let guard = global_guard();` after the block (no memory argument) | same | **passes** — `ok. 1 passed`, one `unused variable: guard` warning — the documented out-of-body limit, exactly as named ("a helper that returns the guard"), R2-2 precedent applies |

Each mutation reverted and hash-verified identical (`c30de825…`) after the
run; the shipped-form runs came from the restored bytes. The checks fail in
order: parse 227:85, macro 239:5, copy expect 249:61, uniqueness 254:5,
stats expect 261:57, stats order 265:5, build expect 270:57, confinement
294:9, count 312:5. Forty-four forms fail closed; one survivor — v1, the
value-escape — passes with the hazard present, which is the finding.

## Convergence judgment

**The structural pin has converged on the spelling and indirection class —
the class the round-6 review named — and its one residual is a value-escape,
not a spelling.** Every textual and token-level disguise of "bind a
memory-derived guard at function scope after the copy block" is closed: the
receiver-respelling family (parens, derefs, blocks, casts, UFCS paths, alias
bindings, read-family names), the whitespace/multiline/spacing variants, the
macro and helper-call indirection, the closure/nested-scope placements, and
this round's chained aliases, shadowing, dead closures, whitelist probes and
slice-robustness probes — 44 executed forms fail closed, 6 executed safe
forms pass, and the confinement rule is sound for every memory-derived guard
because no alias of a memory-derived value can exist outside the block (its
binding references `memory` and is caught). The count check's misses all err
fail-closed except one: an acquisition whose guard value is *consumed* rather
than *bound* (v1). That is the honest boundary the round-6 prescription did
not reach: the pin proves *where* the acquisition sits, not *what happens to
the guard* — and the doc's "the guard that drops at the block's close" claim
requires the latter. It is fixable with a binding-position check (acquisition
must be a `let` initializer), which makes the doc literally true; the
alternative is to extend the documented limit to name the value-escape class.
Neither the spellings nor the semantics are exhausted at this point: no
further textual or structural respelling of the guard-across-build class
survives the pin; the value-escape and the out-of-body helper are the two
remaining mechanisms, one an undocumented finding, one a documented limit.

**The documented limit is honest and acceptable.** The pin's doc names "a
helper that returns the guard, or a caller that takes the copy with it" as
beyond any token pin, and the executed (g1) form — a no-argument helper
reading a static graph — passes exactly as documented, with the R2-2
precedent (a documented limit outside the pin's named fault) properly
applying, as the round-6 record prescribed. The one clause worth a
remediator's line: "the body then contains no acquisition tokens at all" is
imprecise in the additive variant (real acquisition retained beside the
helper call), though the class is named without qualification and the count
check even catches the pure-replacement variant (found 0 → 312:5) that the
doc implies is invisible. M12b-R7-1 (v1) is not the documented limit: its
acquisition is inside the body, and the doc's "exactly one acquisition,
inside the block, drops at the block's close" claim is what it falsifies —
so it sits inside the named fault per the standing P2 convention, no
deferrals.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset), on the final tree:

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.36 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  760, `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs`
  169, `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs`
  119, `cli/tests.rs` 811, `PLAN.md` 735, `Cargo.toml` 62 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`);
  the lock diff touches only the root package's dependency list, which gains
  `syn 2.0.119` — the version already vendored transitively
  (serde_derive/schemars build the same 2.0.x), so no new package enters the
  tree. `cargo test --locked` resolves and runs.
* The seven milestone pins green on the final tree, individually (listed
  under What held), plus the shipped-form pin re-run after the battery.
* `src/memory/view.rs` byte-identical to the recorded hash (`c30de825…`) at
  the end; `git status --porcelain` shows exactly the pre-review set (12
  modified + 13 untracked) plus this record; nothing committed.

## What was executed vs. only read

**Executed.** Forty-six pin runs: the shipped form twice (baseline before the
battery and again after it, on the final tree) and forty-four mutations as
tabled — the nine round-6 forms, the fourteen historical evasions, the six
safe forms, and fifteen round-7 attack probes (alias chains in and out of the
block, shadowing, re-borrows, dead closures, whitelist narrowness and
lookalike, slice cuts by comment/string/doc-comment, unsafe-in-block, the
mem::forget and drop value-destination forms, and the static-graph helper) —
every one applied to a byte copy-restored `view.rs` and `sha256sum`-verified
identical (`c30de825…`) before and after every run; full panic messages
re-captured with line and column for the representative forms (227:85, 239:5,
249:61, 254:5, 261:57, 265:5, 270:57, 294:9, 312:5); warning counts for the
representative runs; the compile of every mutation verified (each mutation
must compile — the whole test target builds); the seven milestone pins
individually; the full suite in a clean env; clippy; fmt; the file-size
count; the lambo pin and the syn version re-confirmed from `Cargo.lock`
(syn 2.0.119, one root-dependency line); lambo's `Memory::graph`/`stats`
signatures and `Graph::new(SessionId)` / `SessionId(pub String)` confirmed
from the pinned checkout for the (m18)/(g1) helper mutations; the R2-2
acceptance precedent read from `m12a-round3.md` (lines 212–218) and the M12a
`of_graph`-pub standing note from `m12a-round6.md` (lines 205–208) to ground
the "limit outside the named fault" distinction.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by the mutation
battery; rounds 1–3 already demonstrated the wedged behaviour on this machine
and no lock code changed). The `--demo` pty interplay (round 1 executed it;
no TUI code changed this round). The measurement harness in
`view_tick_tests.rs` (untouched; no production code changed). The
`ManuallyDrop`/`Box::leak` sibling spellings of v1 (same class, same
mechanism — `Drop` skipped; v1 executed stands for them).

## Notes for M12c

* Both round-6 findings are closed at their site and hold under re-attack:
  M12b-R6-1 (the nine forms → 294:9, every one) and M12b-R6-2 (`!(1 > 2)`
  passes, zero warnings). The alias map, the confinement walk's descent into
  closures, the whitelist's narrowness, and the slice's
  comment/literal-aware truncation all behave as documented; the parse expect
  fails loud (227:85) and the whitelist/stats expect fails closed (261:57).
* The one residual is M12b-R7-1: the pin proves where the acquisition sits,
  not what happens to the guard value — `std::mem::forget(guard)` (equally
  `ManuallyDrop`, `Box::leak`) inside the copy block passes with the read
  lock held across the build and forever. Fix: count an acquisition only when
  it is the initializer of a local binding, making the doc's "the guard's
  binding scope is the block" literally true; or extend the doc's limit to
  name the value-escape class. Verdict REMEDIATE per the round's instruction;
  no deferrals.
* The documented out-of-body limit is honest and acceptable (g1 executed:
  the no-argument static-graph helper passes as documented; R2-2 applies).
  One wording nit for the remediator: the doc's "the body then contains no
  acquisition tokens at all" is imprecise in the additive variant; the class
  itself is named correctly.
* The milestone itself is unchanged and sound: `view.rs` byte-identical to
  the round-1-through-round-6 hash (`c30de825…`, 975 lines), `of_memory`
  holds the guard for exactly one copy, and every pin and gate is green.
  Seven rounds of findings have been about the review pin's coverage, not the
  code; the coverage question now has one named survivor (the value-escape)
  and one documented boundary (the out-of-body helper).
* The check line numbers are unchanged from the round-6 record, except the
  stats expect renders at 261:57 in this reading: parse 227:85, macro 239:5,
  copy expect 249:61, uniqueness 254:5, stats expect 261:57, stats order
  265:5, build expect 270:57, confinement 294:9, count 312:5.
* The tree stays dirty with the implementation + all six remediations +
  Cargo.toml/Cargo.lock + this record, exactly as the orchestrator expects;
  nothing committed.
