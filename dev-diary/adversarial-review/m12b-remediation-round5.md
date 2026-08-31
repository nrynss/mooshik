# M12b round-5 remediation

Remediates the two findings in `m12b-round5.md` — P2 M12b-R5-1 (a
module-level `macro_rules!` whose invocation acquired the graph guard at
function scope after the copy block: the pin slices the body between
`pub fn of_memory` and `fn of_graph`, the `macro_rules!` definition sits
above the slice, and `let guard = grab!(memory);` carries no
`memory.graph().read()` text for any check to see, so the pin passed with
the guard held across the build) and P2 M12b-R5-2 (the no-acquisition
`contains("memory.graph().read()")` over the `close..build` window was
fail-open to whitespace inside the call: `memory.graph().read ();` is valid
Rust, binds a function-scope guard, and escaped the exact-text `contains`).
No deferrals. Base and destination: branch `main` at `709e911`; the tree is
left dirty for the orchestrator, nothing committed. All mutations below were
transient: `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after every run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the
same hash every prior round recorded — the round-5 remediation changed
`view.rs` not at all).

## M12b-R5-1 — the pin now fails closed on macro invocation tokens, and M12b-R5-2 — every acquisition is matched modulo insignificant whitespace

The two fixes are one mechanism: the pin now anchors on the **flattened**
body — `strip_rust_shell` first (comments and literals gone, as before),
then every ASCII whitespace run removed — and it rejects macro invocations
outright.

```rust
    // Comments and literals are stripped (anchors must come from code, never
    // from a comment or a string), then every ASCII whitespace run is
    // flattened away, so the anchors match any whitespace spelling:
    // `memory.graph().read ()` flattens to the same text as
    // `memory.graph().read()`, and `let graph =\n{` to the same text as
    // `let graph = {`. Rust treats the whitespace between tokens as
    // insignificant, so a whitespace variant compiles to the same program
    // while a byte-for-byte anchor would miss it — a spaced acquisition
    // would escape the no-acquisition window below, and a spaced block
    // anchor would shift the open/close containment. Every find, brace
    // count and contains below runs on this flattened text.
    let flat = flatten_whitespace(&strip_rust_shell(body));
    // A macro invocation's expansion is invisible to these text checks: a
    // `grab!(memory)` (whose `macro_rules!` sits above the fn, outside this
    // slice) expands to `memory.graph().read()` and binds a function-scope
    // guard held across the build, yet carries no acquisition text of its
    // own. Every macro invocation spells `!` immediately followed by `(`,
    // `{` or `[`, so the flattened body must contain none — the shipped
    // body has no `!` at all, and one such token anywhere means the checks
    // below cannot see every acquisition.
    assert!(
        !flat
            .as_bytes()
            .windows(2)
            .any(|w| w[0] == b'!' && matches!(w[1], b'(' | b'{' | b'[')),
        "no macro invocation may appear in of_memory's body: an invocation's \
         expansion is invisible to these source-order checks, so it could \
         acquire the graph guard at function scope and hold it across the build"
    );
```

### M12b-R5-1: what was wrong and the fix

The review executed `macro_rules! grab` at module level (outside the pin's
body slice) with `let guard = grab!(memory);` at function scope after the
copy block. Every check resolved against the in-block `inner` acquisition:
order held, a `}` closed the block, the anchor sat at depth 1, containment
held, and the `close..build` window contained no exact-text acquisition —
`ok. 1 passed; 545 filtered out`, while the compiled `grab!(memory)` bound a
`parking_lot::RwLockReadGuard` alive for the whole `of_graph` call.

The fix is the reviewer's prescribed coarse, honest check: the pin cannot
see expansion text, so **any** macro invocation in the scanned region fails
closed. A macro invocation is exactly `!` immediately followed by `(`, `{`
or `[` (Rust's three macro delimiters; whitespace and comments between the
`!` and the delimiter are insignificant, and both are gone from the
flattened text, so `grab ! (memory)` flattens to `grab!(memory)` and trips
the same check). The shipped `of_memory` body contains no `!` at all —
verified against the real code before choosing the check — so the shipped
body accepts it. The rejection is deliberately coarse: a macro invocation
that would expand to an *in-block* acquisition (which would be safe) is
rejected too, because the pin cannot verify what the invocation expands to;
fail-closed means refusing the invisible, not guessing. The doc now claims
exactly this: "it contains no macro invocation (`!` immediately followed by
`(`, `{` or `[`), because a macro's expansion is invisible to these
source-order checks and could acquire the guard with no acquisition text of
its own".

### M12b-R5-2: what was wrong and the fix

The review executed `let guard = memory.graph().read ();` (one space before
the parens) after the block's close: valid Rust, function-scope guard held
across the build, invisible to the exact-text `contains`. The `.find`
anchors were already fail-closed on respelling (a spaced *only* acquisition
panicked the expect), but the `contains` was fail-open — the round-4
record's Notes claim that a whitespace-spelled anchor "cannot create a false
pass" was false for the check that remediation added.

The fix normalizes the whitespace once, up front, for **every** anchor —
not just the no-acquisition window: `flatten_whitespace` removes all ASCII
whitespace from the stripped body, and every `find`, brace count and
`contains` in the pin runs on that text. Consequences, all executed:

* `memory.graph().read ()` flattens to `memory.graph().read()`, so the
  no-acquisition `contains` sees it: mutation (n2) now fails at the
  no-acquisition assert, `300:5`, with the guard-warning exactly as before.
* A spaced acquisition **before** the block is found by the guard anchor
  (no expect panic) and fails containment like any hoisted guard: mutation
  (p), `let guard = memory.graph().read ();` hoisted, fails at `288:5` with
  the containment message, not with "the graph guard is taken".
* A call broken across lines — `memory.\ngraph()\n.read()` — flattens to the
  same text: mutation (o3) fails at `300:5`. An `unsafe {
  memory.graph().read() }` after the close: (o4) fails at `300:5`.
* The block anchor is normalized the same way (`letgraph={`), so a spaced
  or split `let graph =\n{` cannot shift the open/close containment — the
  split-across-lines anchor game, the frontier the round-3/round-4 records
  named, is closed for this pin as a side effect of the same mechanism.

The new helper, documented beside `strip_rust_shell`:

```rust
/// The stripped code with every ASCII whitespace run removed, so the pin's
/// anchors match across any whitespace spelling. Rust treats the whitespace
/// between tokens as insignificant, so `memory.graph().read ()`,
/// `memory.graph()\n.read()` and `memory.graph().read()` compile to the same
/// program — a byte-for-byte anchor would miss the first two — and `let
/// graph =\n{` compiles to the same block as `let graph = {`. Matching on
/// the flattened text keeps the acquisition, block and build anchors (and
/// the no-acquisition window) whitespace-insensitive: a spaced acquisition
/// cannot escape a `contains`, a spaced acquisition before the block is
/// still found by the guard anchor, and a spaced block anchor cannot shift
/// the open/close containment.
fn flatten_whitespace(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    out.extend(code.chars().filter(|c| !c.is_ascii_whitespace()));
    out
}
```

The doc now claims exactly what the checks prove: on comment- and
literal-stripped, whitespace-flattened text, the body contains no macro
invocation; the acquisition, the copy and the build appear in that order; a
`}` closes a block between the copy and the build call; the first
`let graph = {` sits at the body's top level; the first acquisition and the
copy sit between that block's own braces; and no `memory.graph().read()`
lies between the block's close and the build call. "With macro invocations
rejected and the whitespace flattened, every acquisition text is visible to
those checks, so together they put every acquisition inside the copy's
block." It names the module-level `macro_rules!` invocation as what the
no-macro check bites on and the whitespace-spelled hoist as what
containment bites on.

## The proof

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). The pin run on the shipped bytes before the mutations and
again after the final revert. All runs in a clean env (the three DSN
variables unset).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:245:10: the copy's block closes before the build` (the round-1 message, moved with the rework); zero warnings |
| (c) guard binding hoisted above the copy block, block retained | same | **caught** — `panicked at src/memory/view_session_tests.rs:288:5: the guard must be taken inside the copy's block so it drops before the build`; zero warnings |
| (d) decoy block `let graph = { 0 };` before a hoisted guard | same | **caught** — same 288:5 message; one `unused variable: graph` warning |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — the comment is stripped: same 288:5 message, zero warnings |
| (f1) block comment `/* let graph = { 0 }; */` before a hoisted guard | same | **caught** — same 288:5 message, zero warnings |
| (f2) string-brace decoy before a hoisted guard | same | **caught** — same 288:5 message; one `unused variable: graph` warning |
| (f3) raw-string anchor `r#"let graph = { 0 };"#` before a hoisted guard | same | **caught** — same 288:5 message, zero warnings |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok`, 1 passed; no false positive |
| (g) renamed binding hoisted (`let g = …`, copy through `&g`) | same | **caught** — same 288:5 message, zero warnings; the pin anchors on the acquisition call, not the name |
| (i1) guard+copy inside a nested block *inside* the copy block (safe) | same | **passes** — `ok`, 1 passed |
| (i2) copy block carrying a nested `{ let a = 1; a + 1 }` block and a closure before the guard (safe) | same | **passes** — `ok`, 1 passed; one `unused variable: a` warning |
| (j) guard/copy/build folded into one enclosing block (round-2 mutation (g)) | same | **caught** — the anchor sits inside the enclosing block: `panicked at src/memory/view_session_tests.rs:267:5: assertion \`left == right\` failed: the copy's block must be a top-level statement of of_memory, not a block nested inside a closure, fn or match arm` (left: 2, right: 1) |
| (iii1) plain closure decoy before a hoisted guard | same | **caught** — same 267:5 depth message, zero warnings |
| (iii2) plain match-arm decoy before a hoisted guard | same | **caught** — same 267:5 depth message, zero warnings |
| (h1) closure carrying a complete guard+copy before a hoisted guard | same | **caught** — same 267:5 depth message, left: 2, right: 1 (M12b-R4-1 stays closed) |
| (h2) nested fn carrying a complete guard+copy before a hoisted guard | same | **caught** — same 267:5 depth message, left: 2, right: 1 |
| (h3) match arm carrying a complete guard+copy before a hoisted guard | same | **caught** — same 267:5 depth message, left: 3, right: 1 |
| (iv1) string with escaped quotes carrying both anchors before a hoisted guard | same | **caught** — the whole literal incl. `\"` is stripped: same 288:5 message, zero warnings |
| (iv2) `r##"…"##` raw string before a hoisted guard | same | **caught** — same 288:5 message, zero warnings |
| (iv3) byte string `b"…"` before a hoisted guard | same | **caught** — same 288:5 message, zero warnings |
| (iv4) char-literal quote inside a line comment before a hoisted guard | same | **caught** — comments are stripped first: same 288:5 message, zero warnings |
| (iv5) nested `fn helper<'a>` with lifetimes inside `of_memory` (safe) | same | **passes** — `ok`, 1 passed |
| (v) second renamed-binding hoist (`let g2 = …`, copy through `&g2`) | same | **caught** — same 288:5 message, zero warnings |
| (k) top-level decoy statement carrying its own complete guard+copy before a hoisted guard | same | **caught** — the decoy is at depth 1 and its internal copy satisfies order, close and containment, so the no-acquisition check bites: `panicked at src/memory/view_session_tests.rs:300:5: no graph guard may be taken after the copy's block closes: a guard bound there is outside the block and held across the build`; zero warnings |
| (l) extra braces wrapper around the copy block (safe) | same | **passes** — `ok`, 1 passed |
| (m1) `#[allow(unused_variables)]` on the copy statement (safe) | same | **passes** — `ok`, 1 passed |
| (m2) `#[cfg(any())]`-gated block carrying a complete guard+copy before a hoisted guard | same | **caught** — the anchor sits inside the cfg'd block: same 267:5 depth message, zero warnings |
| **(n1) module-level `macro_rules! grab` whose invocation acquires the guard at function scope after the block's close** | same | **caught** — `panicked at src/memory/view_session_tests.rs:218:5: no macro invocation may appear in of_memory's body: an invocation's expansion is invisible to these source-order checks, so it could acquire the graph guard at function scope and hold it across the build`; one `unused variable: guard` warning (P2 M12b-R5-1 closed) |
| **(n2) spaced acquisition `memory.graph().read ();` after the block's close** | same | **caught** — the flattened window contains the acquisition: same 300:5 no-acquisition message; one `unused variable: guard` warning (P2 M12b-R5-2 closed) |
| **(o1) NEW my own evasion: spaced macro invocation `let guard = grab ! (memory);` after the block** | same | **caught** — `grab ! (memory)` flattens to `grab!(memory)`: same 218:5 macro message; one `unused variable: guard` warning |
| **(o2) NEW my own evasion: `let inner = grab!(memory);` inside the copy block** | same | **caught** — macro tokens anywhere in the body fail closed, even one that would expand to an in-block acquisition: same 218:5 macro message, zero warnings |
| **(o3) NEW my own evasion: `memory\n.graph()\n.read();` after the block's close** | same | **caught** — the multiline spine flattens to the exact text: same 300:5 no-acquisition message; one `unused variable: guard` warning |
| **(o4) NEW my own evasion: `let guard = unsafe { memory.graph().read() };` after the block's close** | same | **caught** — the exact acquisition text sits inside the unsafe block: same 300:5 no-acquisition message; two warnings (`unnecessary \`unsafe\` block`, `unused variable: guard`) |
| **(p) NEW spaced hoisted guard `let guard = memory.graph().read ();` above the copy block** | same | **caught** — the spaced acquisition is found by the guard anchor (no expect panic) and fails containment: same 288:5 message, zero warnings |
| (iv-iflet) guard acquired in an `if let` scrutinee after the close, exact text | same | **caught** — same 300:5 no-acquisition message, zero warnings |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. The checks fail in order:
the no-macro assert first (218:5), then the order assert, then the
block-close expect (245:10), then the top-level-depth assert (267:5), then
the containment assert (288:5), then the no-acquisition assert (300:5). All
round-1 through round-4 forms land on the same checks and messages as the
round-4 record (block-close, depth with the same left/right values, 
containment, no-acquisition), with the line numbers moved by the added
checks; every safe form still passes with no false positive.

One further probe, beyond the review's two forms: the parenthesized-receiver
spelling `let guard = (memory.graph()).read();` after the block's close
compiles, binds a function-scope guard held across the build, and **passes
the pin** — the extra parens around the receiver break the exact spine
`memory.graph().read()` even on the flattened text. That is the remaining
textual frontier the review predicted; see Notes.

## Round-4 remediation record corrected

`m12b-remediation-round4.md`'s Notes claimed a whitespace-renamed anchor
"fails the pin's expects closed — it cannot create a false pass". The
round-5 review falsified that for the no-acquisition `contains` (mutation
(n2), a spaced `read ()` after the close, passed with the guard held across
the build). The round-4 record's sentence now carries a parenthetical noting
that the round-5 remediation closes the class for the build pin: the
flattened text *finds* split and whitespace-spelled anchors instead of
missing them, and the no-acquisition window is matched on the same flattened
text; the figures-first pin is unchanged and still anchors on the raw body.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.02 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.27 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  530 (was 474; +56: the doc reworked to claim exactly what the checks
  prove, the flatten step with its comment, the no-macro check with its
  comment, the `let graph = {` → `letgraph={` anchor rename, the flattened
  no-acquisition window comment, and the new `flatten_whitespace` helper),
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

**Executed.** Ninety-two mutation/probe runs against the remediated pin —
the thirty-seven-form battery (a) shipped twice, (b) flat, (c) hoisted,
(d) decoy, (e) line comment, (f1) block comment, (f2) string-brace decoy,
(f3) raw-string, (f4) safe string-brace, (g) renamed hoist, (i1) nested
guard scope, (i2) nested braces + closure, (j) folded, (iii1) closure decoy,
(iii2) match-arm decoy, (h1) closure full-copy, (h2) nested-fn full-copy,
(h3) match-arm full-copy, (iv1) escaped-quote string, (iv2) `r##` raw
string, (iv3) byte string, (iv4) quote in comment, (iv5) lifetimes, (v)
second renamed hoist, (k) top-level decoy-with-guard, (l) extra braces
wrapper, (m1) attribute on the copy, (m2) `#[cfg(any())]`-gated block, (n1)
module macro, (n2) spaced acquisition after close, (o1) spaced macro
invocation, (o2) macro inside the block, (o3) multiline read spine, (o4)
unsafe-wrapped acquisition, (p) spaced hoisted guard, (iv-iflet) if-let
scrutinee — each reverted and `sha256sum`-verified byte-identical
(`c30de825…`) after every run, with the pin run before and after; full panic
messages re-captured for the six representative forms (n1, b, h3, p, k, n2);
the depth assert's left/right values re-captured for (h1), (h2), (h3), (j);
warning counts re-captured for all thirty-seven forms; and the
parenthesized-receiver probe `(memory.graph()).read()` run once (it passes
the pin — the remaining frontier, recorded in Notes). The seven milestone
pins individually. The full suite in a clean env, clippy, fmt, file-size
count, lambo pin re-confirmed from `Cargo.lock`. The round-4 record's Notes
overclaim read and corrected with a parenthetical; the shipped `of_memory`
body read in full and confirmed to contain no `!` (the fact the no-macro
check rests on).

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations
(b)–(iv-iflet); rounds 1–4 already demonstrated the wedged behaviour on this
machine and no lock code changed). The `--demo` pty interplay (round 1
executed it; this remediation touches no TUI code). The measurement harness
in `view_tick_tests.rs` (untouched; no production code changed).

## Notes for M12c

* Both round-5 findings are closed at their site. M12b-R5-1: macro
  invocations fail closed — any `!` immediately followed by `(`, `{` or `[`
  in the flattened body is rejected at the first check, because the pin
  cannot see an invocation's expansion; the shipped body contains no `!`,
  so the check costs nothing on it. The rejection is intentionally coarse:
  a macro that would expand to an *in-block* acquisition is rejected too
  (executed form (o2)) — the pin refuses what it cannot verify, which is
  the fail-closed stance the review prescribed. M12b-R5-2: acquisitions are
  matched modulo insignificant whitespace — the whole body is flattened
  before anchoring, so the no-acquisition `contains`, the guard find, the
  block-close, the depth count and the open/close containment all see
  `memory.graph().read ()` as `memory.graph().read()`, and a spaced
  acquisition before the block is found (containment, not an expect panic).
  Bonus: the `letgraph={` anchor is whitespace-insensitive too, so the
  split-across-lines anchor game the round-3/round-4 records named as the
  frontier is closed for this pin.
* The pin's message line numbers moved with the rework: no-macro assert at
  218:5 (new), block-close expect at 245:10 (was 208:10), depth assert at
  267:5 (was 230:5), containment assert at 288:5 (was 251:5),
  no-acquisition assert at 300:5 (was 261:5). The round-4 record's
  208:10 / 230:5 / 251:5 / 261:5 quotes are historical.
* The one spelling this text pin still cannot see is token-level, not
  whitespace: `(memory.graph()).read()` after the block's close compiles,
  binds a function-scope guard, and passes the pin (executed; the extra
  receiver parens break the exact spine even on flattened text). Any
  string-anchored pin shares this frontier — the honest end-state the
  round-5 review named is a token-level pin built on `syn`, where an
  acquisition is defined structurally instead of by text. Rounds 1–5 have
  closed, in order: the flat form, the comment/literal anchors, the
  guardless decoy, the nested-scope decoy, the top-level decoy, the macro
  invocation and the whitespace variant; the parenthesized receiver is the
  remaining textual case.
* The milestone itself is unchanged: `view.rs` is byte-identical to the
  round-1/round-2/round-3/round-4 hash, `of_memory` holds the guard for
  exactly one copy, and every pin and gate is green. These findings were
  about the review pin's soundness, not the code.
* The tree stays dirty with the implementation + all five remediations +
  this record, exactly as the orchestrator expects; nothing committed.
