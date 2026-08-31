# M12b round 5 — adversarial re-review of the round-4 remediation

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1 through round-4 remediations (10 modified + 9 untracked — the
full expected set, verified identical before and after every mutation).
Scope: the single round-4 finding (P2 M12b-R4-1 — the containment check was
satisfiable by a nested scope carrying its own guard+copy) and the milestone's
continuing hold. All transient mutations reverted and `sha256sum`-verified
identical to the pre-mutation state after each (`src/memory/view.rs`
`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the same
hash every prior round recorded — the round-4 remediation changed `view.rs`
not at all); `git status --porcelain` shows exactly the same 10 modified + 9
untracked as before I started, now with this record beside them. Nothing
committed. Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so every `cargo`
invocation ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 2 × P2.

The round-4 fix is genuine against every form it was built for: the
twenty-one-form proof reproduces verbatim — shipped passes twice (baseline and
after the final revert); flat at 208:10; hoisted, decoy, line-comment,
raw-string and renamed forms at 251:5; the three nested evasions (closure,
nested fn, match arm, each with a hoisted real guard) at the depth assert
230:5 (left 2, 2, 3 vs 1); the top-level decoy-with-own-guard at the
no-acquisition assert 261:5 — and every safe form (string-brace, nested
block+closure inside the copy block, extra-braces wrapper, `const`/`static`
items before the copy) passes, no false positives. The two new checks do what
the record claims. What survives is the same class round-4 closed, expressed
through text the pin cannot see, and it passes the pin with the hazard
present:

```rust
// (n1) module-level macro form — passes, guard held across the build
// (macro_rules! grab defined above of_memory, outside the pin's body slice)
let stats = memory.stats();
let graph = {
    let inner = memory.graph().read();
    ViewData::from_graph(&inner)
};
let guard = grab!(memory); // expands to memory.graph().read() at fn scope
of_graph(&stats, &graph, now)
```

The pin slices the body between `pub fn of_memory` and `fn of_graph`; a
`macro_rules!` defined above the function never enters the slice, so the
acquisition `grab!(memory)` carries no `memory.graph().read()` text the pin
can see. Every check resolves against the in-block `inner` acquisition — order,
block-close, depth 1, containment, no-acquisition — and passes, while the
compiled `let guard = memory.graph().read();` sits at function scope after
the block's close and is dropped only at the end of `of_memory`, after
`of_graph` returns: the writer starvation at a 250 ms tick the copy exists to
prevent, reintroduced by one module-level macro. Executed: `ok. 1 passed;
545 filtered out`, one benign `unused variable: guard` warning. My first
attempt (`grab!()` with `memory` resolved at the definition site) failed to
compile under macro hygiene; the executed form passes `memory` as an
argument.

```rust
// (n2) whitespace-variant form — passes, guard held across the build
let stats = memory.stats();
let graph = {
    let guard = memory.graph().read();
    ViewData::from_graph(&guard)
};
let guard = memory.graph().read (); // one space — invisible to contains()
of_graph(&stats, &graph, now)
```

`memory.graph().read ()` is valid Rust and binds a function-scope
`RwLockReadGuard` held across the build. The pin's `.find` anchors land on the
exact-text in-block acquisition, so order, containment and depth all pass; the
no-acquisition check's `contains("memory.graph().read()")` over the
`close..build` window misses the spaced text (verified on the stripped body:
window `'};\n    let guard = memory.graph().read ();\n    '`, contains →
false) and the pin passes. The round-4 record's Notes claim an anchor "spelled
with different whitespace is not found, which fails the pin's expects closed —
it cannot create a false pass"; that is true of the `.find` anchors (a spaced
guard anchor would panic the expect) but false of the no-acquisition
`contains`, which is fail-open: a spaced acquisition after the close creates
exactly the false pass the record denies. By the same precedent that made
R2-1, R3-1 and R4-1 P2s — a pin passing with the hazard present — both are
P2s, and per the round's instruction there are no deferrals.

## What held up under attack

* **The round-4 twenty-one-form proof reproduces exactly.** Shipped passes
  twice (baseline before the mutations, after the final revert); (b) flat →
  `panicked at src/memory/view_session_tests.rs:208:10: the copy's block
  closes before the build`; (c) hoisted, (d) decoy, (e) line-comment, (f3)
  raw-string, (g) renamed hoist → `panicked at …:251:5: the guard must be
  taken inside the copy's block so it drops before the build`; (h1) closure,
  (h2) nested fn → depth assert at 230:5, `left: 2 / right: 1`; (h3) match
  arm → depth assert at 230:5, `left: 3 / right: 1`; (k) top-level
  decoy-with-own-guard → `panicked at …:261:5: no graph guard may be taken
  after the copy's block closes: a guard bound there is outside the block and
  held across the build`. Each mutation reverted and hash-verified identical
  (`c30de825…`) after each run. Zero false positives on the safe forms.
* **The round-4 checks extend to the round-5 form families the review named.**
  (i) A depth-1 block-expression wrapper `{ let graph = { … } }` around the
  real copy with the guard hoisted — the anchor sits inside the wrapper at
  depth 2 → caught at the depth assert (left 2, right 1). (ii)
  `#[cfg(any())]`-disabled decoys: the block form and the attribute-attached
  top-level `let` form both fail — the block form at depth (nested scope), the
  `let` form at no-acquisition (the cfg'd let is a top-level statement whose
  internal guard satisfies containment while the real hoisted guard after its
  close trips the 261:5 check). (iv) A guard acquired in an `if let`
  scrutinee after the close, exact text — caught at 261:5. (v)
  `const`/`static` items between the body brace and the copy — balanced
  braces, depth stays 1 — passes (safe). (vi) The copy block nested inside
  `unsafe { }` — caught at the depth assert (left 2, right 1); judgment: not
  a safe shipped alternative the pin wrongly rejects — the anchor there is
  genuinely not a top-level statement, the wrapper adds no safety property
  (its contents are all safe operations), and the natural safe spellings
  (copy block at top level; `unsafe` inside the copy block if ever needed)
  pass. (vii) The genuinely safe depth-1 forms — internal guard dropping at
  the block close, nested blocks and closures inside the copy block, the
  extra-braces wrapper — all pass; no false positive.
* **The milestone still holds.** `a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`
  (real sqlite `Memory`, the write lands in the trickle without a keystroke),
  `a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
  `the_figures_are_read_before_the_graph_guard`, and all four M12a regression
  pins (`the_local_database_is_created_and_repaired_private`,
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`) — green
  on the remediated tree, individually run, each `1 passed; 0 failed`.
* **The round-4 remediation touched only the pin.** `view.rs` is
  byte-identical to the recorded hash (`c30de825…`, 975 lines);
  `view_session_tests.rs` is 474 lines (was 435; +39 — the reworked doc, the
  top-level-depth check, the `depth`→`nested` rename, and the
  no-acquisition-after-close check), exactly as the round-4 record describes.
  Nothing else in the tree changed between rounds.
* **The R1-3 structural close stands.** `fn of_graph(stats: &MemoryStats,
  graph: &ViewData, now)` is private; `of_memory` is the only production route
  from a `Memory` to a `Workspace`, so the pins cover every caller there can
  be — re-confirmed by grep over `src/`.
* **Gates and pin.** Full suite in a clean env, clippy, fmt, file-size caps,
  lambo pinned at `4c6fc93` — all as recorded below.

## Findings

### P2

**M12b-R5-1 — a module-level macro_rules! hides the acquisition: the pin
passes while `of_memory` holds a function-scope guard across the build.**

The pin slices the body between `pub fn of_memory` and `fn of_graph` and
proves its invariants on the stripped text of that slice. A `macro_rules!`
defined above `of_memory` never enters the slice, so an invocation inside the
body — `let guard = grab!(memory);` expanding to `memory.graph().read()` — is
an acquisition with no acquisition text. All six checks resolve against the
exact-text acquisition the body does contain (here the `inner` guard inside
the copy block, which is itself safe and drops at the block close): order
holds, a `}` closes the block, the anchor sits at depth 1, the first
acquisition is inside the block, and the `close..build` window is clean.
Executed: `test result: ok. 1 passed; 545 filtered out`, zero warnings (one
`unused variable`), while the compiled `let guard = memory.graph().read();`
is a `parking_lot::RwLockReadGuard` bound at function scope after the block
and alive for the whole `of_graph` call — the writer starvation at a 250 ms
tick the copy exists to prevent. The doc's sentence "Together those put every
acquisition inside the copy's block" is false for this form: the
function-scope acquisition is invisible to every check. Same class as
R2-1/R3-1/R4-1 — a pin passing with the hazard present. (First attempt
failed to compile: macro hygiene resolves `memory` at the definition site;
passing `memory` as the macro argument is the executed, compilable form.)

*Remediation sketch.* The pin cannot see expansion text, so it must fail
closed on macro usage in the scanned region: reject a stripped body
containing any macro invocation (`!` followed by `(`/`{`/`[`), or require the
acquisition's receiver expression to be literally `memory.graph().read()` in
the source (a `grab!(memory)`-style invocation between the body brace and the
build fails either check). The `!`-token check is the coarse, honest one; the
split-across-lines frontier noted below applies to it too.

**M12b-R5-2 — the no-acquisition-after-close check is fail-open to
whitespace inside the acquisition call: `memory.graph().read ()` after the
block's close passes the pin with the guard held across the build.**

The check `!code[close..build].contains("memory.graph().read()")` matches
only the exact text. A valid Rust spelling with one space before the argument
parens — `memory.graph().read ()` — compiles, binds a function-scope guard,
and is invisible to the `contains`; the `.find` anchors are unaffected
because an exact-text acquisition remains inside the copy block. Executed:
`ok. 1 passed; 545 filtered out`, and the stripped-body mechanism verified
(anchor indices: guard 146 in-block, open_brace 124 depth 1, close 210;
window `'};\n    let guard = memory.graph().read ();\n    '`, contains →
false). The round-4 record's Notes claim a whitespace-renamed anchor "fails
the pin's expects closed — it cannot create a false pass"; that holds for the
`.find` anchors (a spaced *only* acquisition panics the expect) but not for
the no-acquisition `contains`, which silently passes — the claim is false for
the check this remediation added.

*Remediation sketch.* Match the acquisition modulo insignificant whitespace
in the `close..build` window — e.g. collapse ASCII whitespace in the window
before `contains`, or search for the receiver-and-method spine
`memory.graph().read` (without the parens) instead of the full call — and,
for symmetry, normalize the guard-anchor `.find` the same way so a spaced
acquisition before the block is caught by containment rather than panicking
the expect.

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`). All
runs under a clean env (the three DSN variables unset).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:208:10: the copy's block closes before the build`, verbatim vs the round-4 record |
| (c) guard binding hoisted above the copy block, block retained | same | **caught** — `panicked at src/memory/view_session_tests.rs:251:5: the guard must be taken inside the copy's block so it drops before the build`, verbatim |
| (d) decoy block `let graph = { 0 };` before a hoisted guard | same | **caught** — same 251:5 message |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — comment stripped; same 251:5 message |
| (f3) raw-string anchor `r#"let graph = { 0 };"#` before a hoisted guard | same | **caught** — same 251:5 message |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok`, 1 passed; no false positive |
| (g) renamed binding hoisted (`let g = memory.graph().read();`, copy through `&g`) | same | **caught** — same 251:5 message; the pin anchors on the acquisition call, not the name |
| (h1) closure carrying a complete guard+copy before a hoisted guard | same | **caught** — `panicked at src/memory/view_session_tests.rs:230:5: assertion \`left == right\` failed: the copy's block must be a top-level statement of of_memory, not a block nested inside a closure, fn or match arm`, left: 2, right: 1 (M12b-R4-1 stays closed) |
| (h2) nested fn carrying a complete guard+copy before a hoisted guard | same | **caught** — same 230:5 depth message, left: 2, right: 1 |
| (h3) match arm carrying a complete guard+copy before a hoisted guard | same | **caught** — same 230:5 depth message, left: 3, right: 1 |
| (k) top-level decoy statement carrying its own guard+copy before a hoisted guard | same | **caught** — the decoy is at depth 1 and its internal copy satisfies order, close and containment, so the no-acquisition check bites: `panicked at src/memory/view_session_tests.rs:261:5: no graph guard may be taken after the copy's block closes: a guard bound there is outside the block and held across the build` |
| **(i) depth-1 block-expression wrapper `{ let graph = { … } }` around the real copy, guard hoisted** | same | **caught** — the wrapper's `{` precedes the anchor: same 230:5 depth message, left: 2, right: 1 |
| **(cfglet) `#[cfg(any())]`-gated top-level `let` carrying its own guard+copy before a hoisted guard** | same | **caught** — the cfg'd `let` is a top-level statement (depth 1) whose internal guard satisfies containment, so the hoisted real guard after its close trips the check: same 261:5 message |
| **(n1) module-level `macro_rules! grab` whose invocation acquires the guard at function scope after the block's close** | same | **SURVIVES** — `ok`, 1 passed, zero warnings, with the real guard held across the build (P2, M12b-R5-1). First attempt (`grab!()` resolving `memory` at the definition site) failed to compile under macro hygiene; the executed form passes `memory` as an argument |
| **(n2) spaced acquisition `memory.graph().read ();` after the block's close** | same | **SURVIVES** — `ok`, 1 passed, one `unused variable` warning, with the real guard held across the build (P2, M12b-R5-2); stripped-body mechanics verified (window `'};\n    let guard = memory.graph().read ();\n    '`, exact-text `contains` → false) |
| (iv-iflet) guard acquired in an `if let` scrutinee after the close, exact text | same | **caught** — same 261:5 message; the exact-text acquisition in the `close..build` window trips the no-acquisition check |
| (vi-unsafe) copy block nested inside an `unsafe { }` at the body's top level, guard hoisted | same | **caught** — the anchor sits inside the unsafe block: same 230:5 depth message, left: 2, right: 1; judged a correct rejection, not a false positive on a safe form (see What held) |
| (v-const) `const _KEEP: usize = { 1 + 1 };` and `static _KEEP2: usize = 2;` between the body brace and the copy (safe) | same | **passes** — `ok`, 1 passed; balanced item braces leave the depth at 1, the guard drops at the block close |
| (vii-safe) copy block carrying a nested `{ let a = 1; a + 1 }` block and a closure before the guard (safe) | same | **passes** — `ok`, 1 passed; nested braces do not confuse the count |
| (l) extra braces wrapper around the copy block (round-4 safe form) | same | **passes** — `ok`, 1 passed; the outer block anchors at depth 1 |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. The checks fail in order,
unchanged from the round-4 record: block-close expect 208:10, depth assert
230:5, containment assert 251:5, no-acquisition assert 261:5. The two new
survivors both make the pin pass with the hazard present — neither fails the
pin's expects; they fail its *coverage*.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.02 s) **+ doc tests passed**, exit 0 — matches
  every prior record's numbers exactly. Lib phase 14.32 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975, `view_session_tests.rs` 474,
  `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs` 169,
  `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs` 119,
  `cli/tests.rs` 811, `PLAN.md` 735 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`).
* The seven milestone pins green on the remediated tree, individually:
  `a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
  `a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
  `the_figures_are_read_before_the_graph_guard`, and the four M12a regression
  pins (`the_local_database_is_created_and_repaired_private`,
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`) — each
  `test result: ok. 1 passed; 0 failed`.

## What was executed vs. only read

**Executed.** Twenty-one mutation runs against the pin — (a) shipped twice,
(b) flat, (c) hoisted, (d) decoy, (e) line-comment, (f3) raw-string, (f4)
safe string-brace, (g) renamed hoist, (h1) closure full-copy, (h2) nested-fn
full-copy, (h3) match-arm full-copy, (k) top-level decoy-with-guard, and the
nine new round-5 forms (i) block-wrapper, (cfglet) cfg'd `let`, (n1) module
macro, (n2) spaced acquisition, (iv-iflet) scrutinee acquisition, (vi-unsafe)
unsafe wrapper, (v-const) const/static items, (vii-safe) nested blocks +
closure, (l) extra braces — each reverted and `sha256sum`-verified
byte-identical (`c30de825…`) after every run, with the pin run before and
after. The n1 compile failure under macro hygiene (first attempt) and the
hygienic fix are recorded above. The stripped-body mechanics of n2 printed
and checked by hand. The seven milestone pins individually. The full suite in
a clean env, clippy, fmt, file-size count, lambo pin re-confirmed from
`Cargo.lock`. The round-4 record's Notes claim about whitespace-renamed
anchors read and falsified for the no-acquisition check by mutation (n2);
every hunk of the full diff read against the current tree; the pin's doc,
strip function and literal scanner read in full; `of_graph`/`of_memory`
callers re-confirmed by grep; the `parking_lot::RwLockReadGuard` return type
of `memory.graph().read()` confirmed from the pinned lambo checkout (line
1254/1406 of `src/memory.rs`).

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations
(b)–(n2); rounds 1–3 already demonstrated the wedged behaviour on this
machine and no lock code changed). The `--demo` pty interplay (round 1
executed it; this round touches no TUI code). The measurement harness in
`view_tick_tests.rs` (untouched; no production code changed). The
`macro_rules!`-rejection remediation sketch above is a suggestion in the
round-2/round-3/round-4 style, not an executed change.

## Notes for M12c

* The M12b milestone itself is sound and ready to ship — `of_memory` holds
  the guard for exactly one copy, `of_graph` takes `ViewData`, and the
  shipped code is unchanged (`view.rs` byte-identical across all five
  rounds). Every finding in this sequence has been about the *review pin's*
  soundness, not the code.
* The round-4 remediation closed the nested-scope class (depth check) and
  the top-level-decoy class (no-acquisition check) for every form whose text
  the pin can see. The round-5 survivors are the same hazard expressed in
  text the pin cannot see or cannot match: a module-level `macro_rules!`
  invocation (n1) and a whitespace variant of the acquisition call (n2).
  Both need the pin to fail closed on macro syntax in the scanned region and
  to match acquisitions modulo insignificant whitespace, respectively — the
  `.find` anchors already fail closed on respelling, and the
  no-acquisition `contains` needs to join them.
* The round-4 record's claim that a whitespace-renamed anchor "cannot create
  a false pass" is true of the `.find` anchors and false of the
  no-acquisition `contains`; the round-4 Notes' frontier sentence should be
  corrected alongside the fix (the remediation record's parenthetical
  convention, as with round 3's overclaim).
* The split-across-lines anchor game (`let graph =\n{`, and now its
  whitespace-in-call cousin) remains the acknowledged frontier of any
  string-anchored pin; the honest end-state is a pin that treats macro
  invocation tokens and whitespace-insensitive call spines as first-class
  text, or a token-level pin built on `syn`.
* The tree stays dirty with the implementation + all four remediations +
  this record, exactly as the orchestrator expects; nothing committed.
