# M12b round-3 remediation

Remediates the single finding in `m12b-round3.md` — P2 M12b-R3-1 (the
block-open check was anchor-foolable: a decoy `let graph = { 0 };` before a
hoisted guard, or a line comment carrying the literal `let graph = {`, passed
all three checks while the `RwLockReadGuard` was bound at function scope and
held across the entire build). No deferrals. Base and destination: branch
`main` at `709e911`; the tree is left dirty for the orchestrator, nothing
committed. All mutations below were transient: `src/memory/view.rs` restored
from a byte copy and `sha256sum`-verified identical to the pre-mutation state
after each run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the same
hash the round-1 and round-2 records both recorded — the round-3 remediation
changed `view.rs` not at all).

## M12b-R3-1 — the pin now anchors on code only, and proves the guard sits between the copy block's own braces

### What was wrong

The round-2 fix anchored the block-open check on the *first* `let graph = {`
in the raw body text. That proves some block-shaped text precedes the guard
text — not that the guard is bound inside the copy's block, which is what the
pin's doc claimed. Two evasions kept the hazard while keeping all three
checks green:

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

Both passed the round-2 pin (executed by the round-3 review, `ok` each) with
the guard alive for the whole `of_graph` call — the writer starvation at a
250 ms tick the copy exists to prevent, reintroduced by one decoy line or one
comment.

### The fix

The pin now reads the body through `strip_rust_shell`, which removes block
comments, line comments, and string/char/byte/raw literals, so anchors and
braces have to come from code — a comment or a literal can never supply
`let graph = {`, `memory.graph().read()`, `ViewData::from_graph`, `of_graph(`
or a stray brace. All checks then run on the stripped text:

1. the order check (`guard < copy < build`) and the block-close check (a `}`
   between the copy and the build call) — kept, same verbatim messages;
2. the new containment check: `open < guard < close`, where `open` is the
   first `let graph = {` on the stripped text (the copy's real block, since
   the decoy is now itself the first anchor) and `close` is the brace-counted
   matching close of that block (strings are already gone from the text, and
   the shipped block contains no braces beyond its own — its contents are
   `let guard = memory.graph().read();` and `ViewData::from_graph(&guard)`,
   neither of which holds a string literal, so the count is exact).

The guard binding must therefore sit between the copy block's own opening
brace and its matching close, which — within a single block, where a binding
cannot escape its closing brace — is what "the guard is bound inside the
copy's block and drops when it closes" actually means:

```rust
    let code = strip_rust_shell(body);
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
    // Source order alone would let the flat form — guard, copy, build, no
    // block — keep the guard alive across the whole build, so the copy's
    // block must close between the copy and the build call.
    code[copy..build]
        .find('}')
        .expect("the copy's block closes before the build");
    // The open and the close alone would let the decoy form — a
    // `let graph = { 0 };` before a guard hoisted to function scope —
    // satisfy the order and the close checks while the guard stays alive
    // across the whole build, so the guard must sit between the copy block's
    // own opening brace and its matching close: open < guard < close.
    let open = code
        .find("let graph = {")
        .expect("the copy is a block, opened before the guard");
    let open_brace = code[open..].find('{').expect("the copy's block opens") + open;
    let mut depth = 1usize;
    let close = code[open_brace + 1..build]
        .char_indices()
        .find_map(|(at, ch)| match ch {
            '{' => {
                depth += 1;
                None
            }
            '}' => {
                depth -= 1;
                (depth == 0).then_some(open_brace + 1 + at)
            }
            _ => None,
        })
        .expect("the copy's block closes before the build");
    assert!(
        open_brace < guard && guard < close,
        "the guard must be taken inside the copy's block so it drops before the build"
    );
```

`strip_rust_shell` handles `/* … */` (non-nested — the shipped code has none),
`//` to end of line (which also covers `///` and `//!`), `"…"` strings and
`'…'` char literals with escapes (`\"`, `\\`, `\u{…}`; a char literal never
spans a line, so a bare lifetime `'a` is left alone), byte strings `b"…"` /
`b'…'`, and raw strings `r"…"`, `r#"…"#`, `br"…"`, `br#"…"#`, `cr"…"`,
`cr#"…"#` with their full `#"…"#` terminator; an unterminated literal
consumes the rest of the body. On the shipped body the scrubber is a no-op on
the code itself (the two `//` comments and the `///` doc block between
`of_memory` and `of_graph` are removed; the surviving code has no literals),
so the anchors land exactly where they did before.

The doc comment now claims exactly what the checks prove: on comment- and
literal-stripped text, the acquisition, the copy and the build appear in that
order; a `}` closes a block between the copy and the build call; and the
copy's real block — the first `let graph = {` — opens before the guard while
its matching close comes after it, so the guard binding sits inside that
block and drops when it closes, before the build runs. It names the flat form
as what the block-close check bites on, the hoisted form as what the
open-before-guard check bites on, and the decoy form as what the
guard-before-close check bites on. (The round-4 review found that sentence
false when the first `let graph = {` belongs to a nested scope — closure,
nested fn, match arm — carrying its own guard+copy, which satisfied these
checks with the real guard hoisted beneath it — M12b-R4-1 — closed by this
remediation's top-level-depth check; the same sentence's survival at depth 1,
a top-level statement carrying its own guard+copy, is closed by the
no-acquisition-after-close check.)

### The proof

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). The pin run on the shipped bytes before the mutations and
again after the final revert.

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test memory::view::session_tests::the_build_runs_against_the_copy_and_not_the_guard ... ok`; `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:198:10: the copy's block closes before the build` (round-1 verbatim message kept); `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 545 filtered out` |
| (c) guard binding hoisted above the copy block, block retained (round-2 mutation X) | same | **caught** — `panicked at src/memory/view_session_tests.rs:223:5: the guard must be taken inside the copy's block so it drops before the build`; `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 545 filtered out` |
| (d) **decoy block**: `let graph = { 0 };` before a hoisted guard, real copy block after it (round-3 mutation (d)) | same | **caught** — the decoy's matching close precedes the guard, so `guard < close` fails: same 223:5 message; one `unused variable: graph` warning; `test result: FAILED. 0 passed; 1 failed` |
| (e) **comment anchor**: line comment containing `let graph = {` before a hoisted guard (round-3 mutation (e)) | same | **caught** — the comment is stripped, so the first anchor is the real block, which opens after the guard: same 223:5 message, zero warnings; `test result: FAILED. 0 passed; 1 failed` |
| (f1) **block-comment anchor**: `/* let graph = { … */` before a hoisted guard (further evasion) | same | **caught** — same 223:5 message, zero warnings; `test result: FAILED. 0 passed; 1 failed` |
| (f2) **string brace decoy**: `let graph = { let _string = "{"; };` before a hoisted guard (further evasion — a naive brace count reads the `{` inside the string, drifts to depth 2 and never finds the matching close) | same | **caught** — the literal is stripped, so the decoy's own close is the matching close and precedes the guard: same 223:5 message; one `unused variable: graph` warning; `test result: FAILED. 0 passed; 1 failed` |
| (f3) **raw-string anchor**: `let _s = r#"let graph = { 0 };"#;` before a hoisted guard (further evasion — a raw string can carry the anchor text verbatim) | same | **caught** — the raw string is stripped: same 223:5 message, zero warnings; `test result: FAILED. 0 passed; 1 failed` |
| (f4) **safe string brace**: `let graph = { let _string = "{"; let guard = …; ViewData::from_graph(&guard) };` — guard correctly bound inside the copy block | same | **passes** — `ok`, 1 passed. The safe form does not false-positive: a naive brace counter would have failed it on the string's `{`, the scrubber removes the literal first |
| (g) renamed binding hoisted (`let g = memory.graph().read();`, copy through `&g`) (round-3 mutation (f)) | same | **caught** — the pin anchors on the acquisition call, not the binding name: same 223:5 message; `test result: FAILED. 0 passed; 1 failed` |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. The round-2 record's "passes
only on the shipped shape" is corrected below; the pin's own doc is now
like-for-like with the checks. The checks fail in order: the order assert
first (the inline-in-call form keeps the round-1 verbatim starvation
message), then the block-close expect (the flat form), then the containment
assert (hoisted, decoy, comment, block-comment, string-brace, raw-string and
renamed forms).

## Round-2 remediation record corrected

`m12b-remediation-round2.md` claimed the widened pin "passes only on the
shipped shape where the block opens before the guard and closes before the
build". The round-3 review falsified the "only" with the decoy-block and
comment-anchor forms; the record's sentence now reads "passes on the shipped
shape where the block opens before the guard and closes before the build",
with a parenthetical noting that the "only" did not survive the round-3
review (M12b-R3-1 — a decoy `let graph = { 0 };` before a hoisted guard and
a line comment carrying `let graph = {` both passed the round-2 checks with
the guard alive across the build), closed by this remediation, which anchors
on comment- and literal-stripped text and requires the guard between the copy
block's own opening brace and its matching close.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches rounds 1–3
  exactly. Lib phase 14.30 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  435 (was 321; +114: the pin doc reworked, the two scrubber helpers
  `strip_rust_shell` and `literal_len`, and the open/close containment
  checks), `view_tests.rs` 871, `view_clock_tests.rs` 292,
  `view_tick_tests.rs` 169, `tui/mod.rs` 807, `tui/app.rs` 317,
  `app_tests.rs` 493, `tui_cmd.rs` 119, `cli/tests.rs` 811, `PLAN.md` 735 —
  all under 1000.
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

**Executed.** All ten mutations against the remediated pin — (a) shipped
twice, (b) flat, (c) hoisted, (d) decoy, (e) line-comment anchor, and the
further evasions (f1) block-comment anchor, (f2) string-brace decoy, (f3)
raw-string anchor, (f4) the safe string-brace form, (g) renamed-binding
hoist — each reverted and `sha256sum`-verified byte-identical (`c30de825…`),
with the pin run before and after. The seven milestone pins individually. The
full suite in a clean env, clippy, fmt, file-size count, lambo pin
re-confirmed from `Cargo.lock`. The round-2 record's overclaim read and
corrected.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations
(b)–(g); rounds 1 and 2 already demonstrated the wedged behaviour on this
machine and no lock code changed). The `--demo` pty interplay (round 1
executed it; this remediation touches no TUI code). The measurement harness
in `view_tick_tests.rs` (untouched; no production code changed).

## Notes for M12c

* The round-4 attack surface the review named — decoy blocks, comments,
  string literals, nested braces, renamed bindings — is now closed: every
  executed form in that list fails the pin, and the safe string-brace form
  does not false-positive. The acknowledged frontier of any string-anchored
  pin remains the split-across-lines anchor game (`let graph =\n{`), which
  `strip_rust_shell` cannot help with and which the round-3 review already
  named.
* The pin's message line numbers moved with the rework (block-close expect at
  198:10, containment assert at 223:5) — the round-2 record's 198:5 / 191:10
  quotes are historical.
* The milestone itself is unchanged: `view.rs` is byte-identical to the
  round-1/round-2 hash, `of_memory` holds the guard for exactly one copy, and
  every pin and gate is green. This finding was about the review pin's
  soundness, not the code.
* The tree stays dirty with the implementation + all three remediations +
  this record, exactly as the orchestrator expects; nothing committed.
