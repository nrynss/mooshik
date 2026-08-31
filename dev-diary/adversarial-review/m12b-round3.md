# M12b round 3 — adversarial re-review of the round-2 remediation

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1 and round-2 remediations (10 modified + 5 untracked — the full
expected set, verified identical before and after every mutation). Scope: the
single round-2 finding (P2 M12b-R2-1 — the guard's scope was still invisible to
the pin) and the milestone's continuing hold. All transient mutations reverted
and `sha256sum`-verified identical to the pre-mutation state after each
(`src/memory/view.rs` `c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`,
the same hash the round-1 and round-2 records both recorded — the round-2
remediation changed `view.rs` not at all, confirming its "view.rs 975
(unchanged)" claim); `git status --porcelain` shows exactly the same 10 modified
+ 5 untracked as before I started, now with this record beside them. Nothing
committed. Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so every `cargo`
invocation ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 1 × P2.

The round-2 fix is genuine against the two forms it was built for and nothing
more: the hoisted guard and the flat form both fail with exactly the quoted
messages, the shipped block-scoped form passes, the renamed-binding hoist fails
identically (the pin anchors on the acquisition call, not the binding name, and
the reviewer's mutation shape does reach a held guard), the build-under-guard
block fails the block-close check (confirming the pin's brace logic is sound
against the language *given honest anchors*), and the milestone holds — all
seven milestone/regression pins green, full suite green, R1-3 structural close
standing, and the round-2 remediation touched only the pin's doc, the pin's
checks and the round-1 record's overclaim correction. What survives is the same
class of hole round-1 and round-2 both flagged: the pin's third check is
**anchor-foolable**. The block-open check proves "some `let graph = {` text
appears before the guard text", not "the guard is bound inside the copy's
block" — the pin's own doc claims the latter. Two new evasions — a decoy block,
and a comment carrying the anchor string — pass the pin while the
`RwLockReadGuard` is bound at function scope and held for the entire build,
executed, one run each, `ok`:

```rust
// decoy-block form — passes, guard held across the build
let stats = memory.stats();
let graph = { 0 };
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)

// comment-anchor form — passes, guard held across the build, zero warnings
let stats = memory.stats();
// let graph = { the copy's block opens before the guard is taken
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

In both, `guard < copy < build` holds, a `}` sits between the copy and the
build, and the first `let graph = {` in the body precedes the guard — so all
three checks pass — while the guard is dropped only at the end of `of_memory`,
after `of_graph` returns: the writer starvation at a 250 ms tick the copy
exists to prevent, reintroduced by one decoy line or one comment. The pin's doc
— "the copy's block opens before the guard is taken, so the guard is bound
inside the block and dropped when it closes" — is false for both forms, and the
round-2 record's "passes only on the shipped shape" overstates the check in the
same way round-2 found the round-1 record's "of nothing the pin accepts" did.
By the round-2 precedent that made the hoisted form a P2 (a pin passing with
the hazard present), this is a P2, and per the round's instruction there are no
deferrals.

## What held up under attack

* **The round-2 documented mutations fail exactly as quoted.** Hoisted form →
`panicked at src/memory/view_session_tests.rs:198:5: the guard must be taken
inside the copy's block so it drops before the build` (the round-2 record's
verbatim message); flat form → `panicked at
src/memory/view_session_tests.rs:191:10: the copy's block closes before the
build` (round-1's verbatim message). Shipped form green, twice (baseline before
the mutations and after the final revert). Each mutation reverted and
hash-verified.
* **The pin checks the acquisition, not the binding name.** Hoisting a renamed
binding (`let g = memory.graph().read();`, copy through `&g`) fails at the same
198:5 block-open assert with the same message — the anchor is
`memory.graph().read()`, so a different spelling cannot dodge the check, and
the reviewer's hoisted-form mutation genuinely leaves a held `RwLockReadGuard`
alive across the build (that is exactly why it fails).
* **The brace logic is sound against the language, given honest anchors.**
The only Rust-legal way to extend a guard's scope past the build textually is
to enclose the build in the guard's enclosing block; mutation (g) — the whole
guard/copy/build folded into one `{ … }` — fails at 191:10 because no `}`
then sits between the copy and the call. Within a single block, a binding
cannot escape its closing brace, so "block opens before guard, block closes
before build" does imply "guard drops before build" — when both braces belong
to the block the anchors name. The residual hole is the anchors themselves
(see the finding).
* **The milestone still holds.** `a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`
(real sqlite `Memory`, the write lands in the trickle without a keystroke),
`a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
`the_figures_are_read_before_the_graph_guard`, and all four M12a regression
pins (`the_local_database_is_created_and_repaired_private`,
`the_scratch_sandbox_and_script_stay_private`,
`two_sandboxes_opened_in_the_same_instant_are_two_directories`,
`a_termination_signal_disposition_is_restored_after_the_session`) — green on
the remediated tree, individually run.
* **The round-2 remediation touched only the pin, its doc, and the round-1
record's correction.** `view.rs` is byte-identical to the round-1
post-remediation hash (`c30de825…`, 975 lines); `view_session_tests.rs` is 321
lines — the round-2 record's 308 + 3 doc + 10 check — and the diff shows the
round-2 delta is exactly the block-open comment (3 lines), the
`let block = …` anchor (4 lines) and the assert (3 lines); the round-1
remediation record now carries the "(The block close alone did not yet cover
every shape the pin accepted: round 2 found the hoisted-guard form passing it —
M12b-R2-1 — closed by the round-2 remediation.)" parenthetical. Nothing else
in the tree changed between rounds; every hunk re-read against the current
tree.
* **The R1-3 structural close stands.** `fn of_graph(stats: &MemoryStats,
graph: &ViewData, now)` is private; grep over `src/` shows its only non-test
caller is `of_memory` (view.rs:243), and `of_memory`'s only production callers
are `tui_cmd::live`'s two (tui_cmd.rs:83-84 — first model + tick closure).
* **Gates and pin.** Full suite in a clean env, clippy, fmt, file-size caps,
lambo pinned at `4c6fc93` — all as recorded below.

## Findings

### P2

**M12b-R3-1 — the block-open check is anchor-foolable: a decoy block or a
comment carrying `let graph = {` passes the pin with the guard held across the
build.**

The round-2 fix adds `block < guard`, anchoring on the *first* occurrence of
`let graph = {` in `of_memory`'s body. That proves some block-shaped text
opens before the guard text — not that the guard is bound inside the copy's
block, which is what the pin's doc claims. Two mutations put the hazard back
while keeping all three checks green:

```rust
// (d) decoy block: the first `let graph = {` is a shadowed decoy that closes
// before the guard; the real copy block opens after the guard, which is bound
// at function scope and dropped only at the end of of_memory.
let stats = memory.stats();
let graph = { 0 };
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

```rust
// (e) comment anchor: the first `let graph = {` is inside a line comment;
// the guard binding below it is at function scope, held across the build.
let stats = memory.stats();
// let graph = { the copy's block opens before the guard is taken
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

Both compile; (d) emits one `unused variable: graph` warning (the shadowed
decoy), (e) compiles clean with zero warnings. Both pass the pin — executed,
one run each, `ok`, 1 passed — while the `RwLockReadGuard` is alive for the
whole `of_graph` call, the writer starvation at a 250 ms tick the copy exists
to prevent. The pin's doc sentence "the copy's block opens before the guard is
taken, so the guard is bound inside the block and dropped when it closes" is
false for both, and the round-2 record's claim that the pin "passes only on the
shipped shape" is the same overclaim round-2 itself called the core defect in
the round-1 record. The checks never verify that the block that opens before
the guard is the block that closes after the copy, nor that the anchor text is
code rather than a comment.

*Remediation.* Strip line comments (the file's only comment style around these
anchors) from `body` before anchoring, and require the guard to sit between the
copy block's own opening brace and its closing brace — `open < guard < close`
on the same stripped text — in the style the pin already uses. Sketch:

```rust
    // Anchors must be code, not comments: a comment carrying `let graph = {`
    // would make the block-open check pass against text that is not the copy
    // block. Strip line comments, then require the guard to sit between the
    // copy block's own braces — open < guard < close on the same text — which
    // is what Rust's scope rule actually binds on.
    let mut code = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find("//") {
        code.push_str(&rest[..at]);
        rest = &rest[at..];
        let newline = rest.find('\n').map_or(rest.len(), |n| n + 1);
        rest = &rest[newline..];
    }
    code.push_str(rest);
    let guard = code
        .find("memory.graph().read()")
        .expect("the graph guard is taken");
    let copy = code
        .find("ViewData::from_graph")
        .expect("the graph is copied out from under the guard");
    let build = code.find("of_graph(").expect("the build follows the copy");
    assert!(
        guard < copy && copy < build,
        "of_memory must copy the graph out under the guard and build from the copy: \
         the current order holds the guard across the build, which starves a writer \
         at a 250 ms tick"
    );
    let open = code
        .find("let graph = {")
        .expect("the copy is a block, opened before the guard");
    let close = code[open..build]
        .find('}')
        .expect("the copy's block closes before the build")
        + open;
    assert!(
        open < guard && guard < close,
        "the guard must be taken inside the copy's block so it drops before the build"
    );
```

This fails the hoisted form (guard before `open`), the flat form (no block to
anchor or close), the decoy-block form (the decoy's `}` closes before the
guard, so `guard < close` fails), the comment form (the comment is stripped, so
`open` is the real block and the hoisted guard precedes it), the renamed-binding
hoist, and the build-under-guard block, and passes the shipped shape. Block
comments and string literals deserve the same treatment if the style ever
admits them around these anchors; the split-across-lines anchor game
(`let graph =\n{`) remains the acknowledged frontier of any string-anchored
pin.

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the
round-1/round-2 recorded hash).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) guard binding hoisted above the copy block, block retained (round-2 mutation (b)) | `the_build_runs_against_the_copy_and_not_the_guard` | **caught** — `panicked at src/memory/view_session_tests.rs:198:5: the guard must be taken inside the copy's block so it drops before the build`, verbatim vs the round-2 record |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:191:10: the copy's block closes before the build`, verbatim |
| (c) shipped block-scoped form | same | **passes** — twice: baseline before the mutations and after the final revert |
| (d) **decoy block**: `let graph = { 0 };` before a hoisted guard, real copy block after it | same | **SURVIVES** — 1 run, 1 pass, with the guard held across the build (P2, M12b-R3-1) |
| (e) **comment anchor**: line comment containing `let graph = {` before a hoisted guard | same | **SURVIVES** — 1 run, 1 pass, zero warnings, guard held across the build (P2, M12b-R3-1) |
| (f) renamed binding hoisted (`let g = memory.graph().read();`, copy through `&g`) | same | **caught** — same 198:5 block-open message; the pin anchors on the acquisition call, not the name |
| (g) guard/copy/build folded into one enclosing block (the only textual way to extend guard scope past the build) | same | **caught** — same 191:10 block-close message; the pin's brace logic is sound against the language given honest anchors |

Each mutation reverted and hash-verified identical; the shipped-form runs came
from the restored bytes. The doc-comment review: the pin's doc names the
hoisted-form hazard accurately and claims "the guard is bound inside the block
and dropped when it closes" — exactly the claim mutations (d) and (e) falsify.

## Gates

Run by me at the end, in a clean env (all three DSN variables unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.02 s) **+ 0 doc**, exit 0. Matches all prior
  records' numbers exactly. Lib phase 14.45 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975, `view_session_tests.rs` 321,
  `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs` 169,
  `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs` 119,
  `cli/tests.rs` 811, `PLAN.md` 735 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`).

## What was executed vs. only read

**Executed.** All seven mutations (a–g) against the pin, each reverted and
`sha256sum`-verified, with the pin run before, after each, and after the final
revert. The headline milestone pins (`a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`,
`a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`), the figures
pin, and all four M12a regression pins — individually. The full suite in a
clean env, clippy, fmt, file-size count, lambo pin re-confirmed from
`Cargo.lock`. The round-2 delta (pin doc +3, checks +10, round-1 record's
parenthetical) confirmed by diff and line counts; the round-2 "view.rs
unchanged" claim confirmed by hash equality with the round-1 record's hash;
`of_graph`/`of_memory` callers enumerated by grep; every hunk of the full diff
read against the current tree.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations (a),
(b), (d), (e), (f), (g); rounds 1 and 2 already demonstrated the wedged
behaviour on this machine and no lock code changed). The `--demo` pty interplay
(round 1 executed it; the round-2 remediation changed no TUI code and `--demo`
still passes `None` through the unchanged `draw`). The measurement harness in
`view_tick_tests.rs` (untouched by the round-2 remediation; the budget numbers
were re-verified in rounds 1 and 2 and no production code changed since).

## Notes for M12c

* The M12b milestone itself is sound and ready to ship — the shipped
  `of_memory` holds the guard for exactly one copy, `of_graph` takes `ViewData`
  and is unreachable with a guard held, and every pin and gate is green. The
  round-3 finding is about the *review pin's* soundness, not the code: if the
  orchestrator accepts the string-anchored pin tradition as bounded, the
  comment-strip + brace-containment check above closes (d) and (e) in the same
  style; if not, the discipline is already enforced by construction
  (`of_graph`'s signature) and the pin's remaining value is regression
  detection for the copy's *position*, which the order assert alone covers.
* The two live-Cloud/print-only ignored tests remain pre-existing and
  unrelated.
* The tree stays dirty with the implementation + both remediations + this
  record, exactly as the orchestrator expects; nothing committed.
