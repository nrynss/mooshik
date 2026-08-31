# M12b round-4 remediation

Remediates the single finding in `m12b-round4.md` — P2 M12b-R4-1 (the
containment check was satisfiable by a nested scope — closure, nested fn,
match arm — carrying its own complete guard+copy: all four checks ran against
the nested block's internal copy while `of_memory`'s real guard sat at
function scope and was held across the entire build). No deferrals. Base and
destination: branch `main` at `709e911`; the tree is left dirty for the
orchestrator, nothing committed. All mutations below were transient:
`src/memory/view.rs` restored from a byte copy and `sha256sum`-verified
identical to the pre-mutation state after each run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the
same hash every prior round recorded — the round-4 remediation changed
`view.rs` not at all).

## M12b-R4-1 — the pin now requires the copy's block to be a top-level statement, and proves every guard is acquired inside it

### What was wrong

The round-3 fix anchored the open/close containment on the *first*
`let graph = {` in the stripped body and required `open < guard < close` on
that block. That is sound against a *guardless* decoy — the decoy's close
precedes the hoisted guard, so `guard < close` fails — but the anchor can be
satisfied by a nested scope that contains its own complete guard+copy. Three
executed round-4 forms passed every check while the real guard was hoisted to
function scope and held for the whole `of_graph` call:

```rust
// (h1) closure form: the never-called `_safe` closure's internal copy
// satisfies every anchor; the real guard below it is held across the build.
let stats = memory.stats();
let _safe = || {
    let graph = {
        let guard = memory.graph().read();
        ViewData::from_graph(&guard)
    };
    graph
};
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

```rust
// (h2) nested-fn form: same, with a nested `fn safe_copy` carrying the
// guard+copy pattern.
let stats = memory.stats();
fn safe_copy(memory: &Memory) -> ViewData {
    let graph = {
        let guard = memory.graph().read();
        ViewData::from_graph(&guard)
    };
    graph
}
let _ = safe_copy;
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

```rust
// (h3) match-arm form: same, with the pattern inside a never-taken arm.
let stats = memory.stats();
let _n = match 1 {
    1 => {
        let graph = {
            let guard = memory.graph().read();
            ViewData::from_graph(&guard)
        };
        let _ = graph;
    }
    _ => {}
};
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

In all three, the first `let graph = {` is the nested scope's internal block:
its guard sits between its own braces, its close precedes the build, and
`guard < copy < build` holds — so every check passed while the function-scope
guard the build actually ran under dropped only at the end of `of_memory`.

### The fix

Two new checks, added to the pin after the open/close anchors:

1. **Top-level-depth check.** The copy's block is a statement of `of_memory`'s
   body, so exactly one unmatched `{` — the body brace itself — may precede
   its opening brace on the stripped text (balanced constructs before it, a
   block or an `if`, leave the count unchanged). A closure, nested fn, match
   arm, `if`/`while` block or `#[cfg]`-gated block puts the anchor at depth
   ≥ 2 and is rejected. This is the reviewer's prescribed check, verbatim in
   effect and message.

2. **No-acquisition-after-close check.** The depth check alone still admits a
   *top-level* decoy — a statement of the body carrying its own complete
   guard+copy — which sits at depth 1 and satisfies the order, close and
   containment checks while the real guard is acquired after the block at
   function scope and held across the build (my own new evasion, form (k)
   below, executed: it passed the reviewer's bare prescription and fails only
   with this second check). The block is the copy's only if no
   `memory.graph().read()` text lies between its close and the build call.

Together with the kept checks (stripped-text order `guard < copy < build`; a
`}` closing a block between the copy and the build; the first acquisition and
the copy between the block's own braces), the two new checks put **every**
`memory.graph().read()` in the body inside the copy's block: any acquisition
before the block would be the first and fails containment, and the
no-acquisition check rules out any after the close. A guard binding created
inside the block drops when the block closes, so none is alive at the build.

```rust
    let open_brace = code[open..].find('{').expect("the copy's block opens") + open;
    // The first `let graph = {` can belong to a nested scope — a closure, a
    // nested fn, a match arm — carrying its own guard+copy, which satisfies
    // the order, close and containment checks while of_memory's real guard
    // sits at function scope, held across the build. The copy's block is a
    // statement of of_memory's body, so exactly one unmatched `{` — the body
    // brace itself — may precede its opening brace on the stripped text
    // (balanced constructs before it, a block or an if, leave the count
    // unchanged).
    let depth = |i: usize| {
        code[..i].bytes().filter(|&b| b == b'{').count()
            - code[..i].bytes().filter(|&b| b == b'}').count()
    };
    assert_eq!(
        depth(open_brace),
        1,
        "the copy's block must be a top-level statement of of_memory, not a \
         block nested inside a closure, fn or match arm"
    );
    let mut nested = 1usize;
    let close = code[open_brace + 1..build]
        .char_indices()
        .find_map(|(at, ch)| match ch {
            '{' => {
                nested += 1;
                None
            }
            '}' => {
                nested -= 1;
                (nested == 0).then_some(open_brace + 1 + at)
            }
            _ => None,
        })
        .expect("the copy's block closes before the build");
    assert!(
        open_brace < guard && guard < close,
        "the guard must be taken inside the copy's block so it drops before the build"
    );
    // The containment check alone would let a *top-level* decoy — a
    // statement of the body carrying its own guard+copy — satisfy every
    // check so far while the real guard is acquired after the block at
    // function scope and held across the build. The block is the copy's only
    // if no guard is acquired between its close and the build: an
    // acquisition there is bound outside the block and outlives it.
    assert!(
        !code[close..build].contains("memory.graph().read()"),
        "no graph guard may be taken after the copy's block closes: a guard \
         bound there is outside the block and held across the build"
    );
```

The shipped body's depth accounting is exact: before the copy block's opening
brace the stripped text has `pub fn of_memory<Tz: TimeZone>(…) -> Workspace {`
— exactly one `{`, the body brace — and `let stats = memory.stats();`, which
has no braces, so the anchor sits at depth 1. (h1)/(h2) sit at depth 2 (the
closure's/fn's brace precedes the anchor), (h3) at depth 3 (match and arm
braces precede it), and every safe nested form — a guard in a nested scope
*inside* the copy block, nested braces and a closure inside the copy block —
anchors on the outer block at depth 1 and passes.

The doc comment now claims exactly what the checks prove: on comment- and
literal-stripped text, the acquisition, the copy and the build appear in that
order; a `}` closes a block between the copy and the build call; the first
`let graph = {` sits at the body's top level — exactly one unmatched `{`,
`of_memory`'s own body brace, precedes its opening brace, so the block is a
statement of the body and not a block nested in a closure, a nested fn or a
match arm; the first acquisition and the copy sit between that block's own
braces; and no `memory.graph().read()` lies between the block's close and the
build call. Together those put every acquisition inside the copy's block, so
the guard binding drops when the block closes, before the build runs. It
names the flat form as what the block-close check bites on, the hoisted,
decoy, comment, literal and renamed forms as what the open-before-guard and
guard-before-close checks bite on, the nested-scope forms as what the
top-level-depth check bites on, and the top-level decoy-with-guard as what
the no-acquisition-after-close check bites on.

### The proof

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). The pin run on the shipped bytes before the mutations and
again after the final revert. All runs in a clean env (the three DSN
variables unset).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:208:10: the copy's block closes before the build` (the round-1 message, moved with the rework); `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 545 filtered out` |
| (c) guard binding hoisted above the copy block, block retained | same | **caught** — `panicked at src/memory/view_session_tests.rs:251:5: the guard must be taken inside the copy's block so it drops before the build`; `test result: FAILED. 0 passed; 1 failed` |
| (d) decoy block `let graph = { 0 };` before a hoisted guard | same | **caught** — the decoy's matching close precedes the guard, so `guard < close` fails: same 251:5 message; one `unused variable: graph` warning; `test result: FAILED. 0 passed; 1 failed` |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — the comment is stripped, so the first anchor is the real block, which opens after the guard: same 251:5 message, zero warnings; `test result: FAILED. 0 passed; 1 failed` |
| (f1) block comment `/* let graph = { 0 }; */` before a hoisted guard | same | **caught** — same 251:5 message, zero warnings; `test result: FAILED. 0 passed; 1 failed` |
| (f2) string-brace decoy `let graph = { let _string = "{"; };` before a hoisted guard | same | **caught** — the literal is stripped, so the decoy's own close precedes the guard: same 251:5 message; one `unused variable: graph` warning; `test result: FAILED. 0 passed; 1 failed` |
| (f3) raw-string anchor `r#"let graph = { 0 };"#` before a hoisted guard | same | **caught** — the raw string is stripped: same 251:5 message, zero warnings; `test result: FAILED. 0 passed; 1 failed` |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok`, 1 passed; no false positive |
| (g) renamed binding hoisted (`let g = memory.graph().read();`, copy through `&g`) | same | **caught** — the pin anchors on the acquisition call, not the name: same 251:5 message; `test result: FAILED. 0 passed; 1 failed` |
| (i1) guard+copy inside a nested block *inside* the copy block — guard drops at the nested close, before the build (safe) | same | **passes** — `ok`, 1 passed; the outer block is the anchor at depth 1 |
| (i2) copy block carrying a nested `{ let a = 1; a + 1 }` block and a closure before the guard (safe) | same | **passes** — `ok`, 1 passed; nested braces do not confuse the count; one `unused variable: a` warning |
| (j) guard/copy/build folded into one enclosing block (round-2 mutation (g)) | same | **caught** — the copy block sits inside the enclosing block: `panicked at src/memory/view_session_tests.rs:230:5: assertion \`left == right\` failed: the copy's block must be a top-level statement of of_memory, not a block nested inside a closure, fn or match arm` (left: 2, right: 1); `test result: FAILED. 0 passed; 1 failed` |
| (iii1) plain closure decoy `\| \| { let graph = { 1 }; graph }` before a hoisted guard | same | **caught** — the anchor sits inside the closure: same 230:5 depth message (left: 2, right: 1); `test result: FAILED. 0 passed; 1 failed` |
| (iii2) plain match-arm decoy before a hoisted guard | same | **caught** — the anchor sits inside the arm: same 230:5 depth message; `test result: FAILED. 0 passed; 1 failed` |
| **(h1) closure carrying a complete guard+copy before a hoisted guard** | same | **caught** — `panicked at src/memory/view_session_tests.rs:230:5: assertion \`left == right\` failed: the copy's block must be a top-level statement of of_memory, not a block nested inside a closure, fn or match arm`, left: 2, right: 1 (P2 M12b-R4-1 closed) |
| **(h2) nested fn carrying a complete guard+copy before a hoisted guard** | same | **caught** — same 230:5 depth message, left: 2, right: 1 |
| **(h3) match arm carrying a complete guard+copy before a hoisted guard** | same | **caught** — same 230:5 depth message, left: 3, right: 1 |
| (iv1) string with escaped quotes carrying both anchors before a hoisted guard | same | **caught** — the whole literal incl. `\"` is stripped: same 251:5 message, zero warnings |
| (iv2) `r##"…"##` raw string with two hashes before a hoisted guard | same | **caught** — same 251:5 message, zero warnings |
| (iv3) byte string `b"…"` before a hoisted guard | same | **caught** — same 251:5 message, zero warnings |
| (iv4) char-literal quote inside a line comment before a hoisted guard | same | **caught** — comments are stripped before literals are considered: same 251:5 message, zero warnings |
| (iv5) nested `fn helper<'a>` with lifetimes inside `of_memory` (safe form) | same | **passes** — `ok`, 1 passed; a bare lifetime is left alone (line-bounded), the mangling the stripper does produce touches no anchor or brace here, and the safe fn has no guard+copy of its own to anchor on |
| (v) second renamed-binding hoist (`let g = …`, copy through `&g`) | same | **caught** — same 251:5 message |
| **(k) NEW depth-1 evasion: a top-level statement carrying its own complete guard+copy before a hoisted guard** | same | **caught** — the block is at depth 1 and its internal copy satisfies order, close and containment, so the no-acquisition check bites: `panicked at src/memory/view_session_tests.rs:261:5: no graph guard may be taken after the copy's block closes: a guard bound there is outside the block and held across the build`; one `unused variable: graph` warning; `test result: FAILED. 0 passed; 1 failed` |
| (l) NEW safe form: the copy wrapped in one extra `{ }` layer | same | **passes** — `ok`, 1 passed; the outer block anchors at depth 1 and the guard drops at the inner block's close, before the build — accepted, not false-failed |
| (m1) NEW safe form: `#[allow(unused_variables)]` on the copy statement (attributes on a `let` are legal Rust) | same | **passes** — `ok`, 1 passed; the attribute adds no brace, the anchor stays at depth 1, and the guard is bound inside the block — safe |
| (m2) NEW evasion: `#[cfg(any())]`-gated block carrying a complete guard+copy before a hoisted guard | same | **caught** — the cfg'd block is itself a nested scope: same 230:5 depth message; `test result: FAILED. 0 passed; 1 failed` |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. The checks fail in order: the
order assert first (the inline-in-call form keeps the round-1 verbatim
starvation message), then the block-close expect (the flat form, now at
208:10), then the top-level-depth assert (nested scopes, 230:5), then the
containment assert (hoisted, decoy, comment, literal and renamed forms,
251:5), then the no-acquisition assert (the top-level decoy-with-guard,
261:5). Three forms that the round-4 review caught at 223:5 — the plain
closure decoy (iii1), the plain match-arm decoy (iii2) and the folded
build-under-guard block (j) — now fail earlier at the depth assert, because
their anchors sit inside nested scopes; the message changed but the rejection
is the same and the depth message is the more precise diagnosis.

## Round-3 remediation record corrected

`m12b-remediation-round3.md` repeated the round-3 pin doc's claim that "the
copy's real block — the first `let graph = {` — opens before the guard while
its matching close comes after it, so the guard binding sits inside that
block and drops when it closes, before the build runs". The round-4 review
falsified that sentence with the nested-scope forms (h1)–(h3), and the
record's sentence now carries a parenthetical noting that the first
`let graph = {` can belong to a nested scope or a top-level decoy carrying
its own guard+copy, closed by this remediation's top-level-depth and
no-acquisition-after-close checks.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.02 s) **+ 0 doc**, exit 0 — matches rounds 1–4
  exactly. Lib phase 14.33 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  474 (was 435; +39: the doc reworked to claim exactly what the checks prove,
  the top-level-depth check with its comment, the `depth`→`nested` rename of
  the close-scan counter, and the no-acquisition-after-close check),
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

**Executed.** Twenty-eight mutation runs against the remediated pin — (a)
shipped twice, (b) flat, (c) hoisted, (d) decoy, (e) line-comment, (f1)
block-comment, (f2) string-brace decoy, (f3) raw-string, (f4) safe
string-brace, (g) renamed binding, (i1) nested guard scope, (i2) nested
braces + closure, (j) folded build under guard, (iii1) closure decoy, (iii2)
match-arm decoy, (h1) closure full-copy, (h2) nested-fn full-copy, (h3)
match-arm full-copy, (iv1) escaped-quote string, (iv2) `r##` raw string, (iv3)
byte string, (iv4) quote in comment, (iv5) lifetimes, (v) second renamed
hoist, and the three new forms (k) top-level decoy-with-guard, (l) extra
braces wrapper, (m1) attribute on the copy, (m2) `#[cfg(any())]`-gated block
decoy — each reverted and `sha256sum`-verified byte-identical (`c30de825…`)
after every run, with the pin run before and after. The seven milestone pins
individually. The full suite in a clean env, clippy, fmt, file-size count,
lambo pin re-confirmed from `Cargo.lock`. The round-3 record's repeated
overclaim read and corrected with a parenthetical.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations
(b)–(m2); rounds 1–3 already demonstrated the wedged behaviour on this
machine and no lock code changed). The `--demo` pty interplay (round 1
executed it; this remediation touches no TUI code). The measurement harness
in `view_tick_tests.rs` (untouched; no production code changed).

## Notes for M12c

* The round-5 attack surface the review named — nested scopes (closure,
  nested fn, match arm) carrying their own guard+copy — is now closed: all
  three executed forms fail the pin at the top-level-depth assert, and the
  depth-1 refinement of the same class (a top-level statement carrying its
  own guard+copy) fails at the no-acquisition assert. Every safe form tried —
  guard in a nested scope, nested braces inside the copy block, the extra
  braces wrapper, the attributed copy — still passes; the safe forms do not
  false-positive.
* The pin's message line numbers moved with the rework: block-close expect at
  208:10 (was 198:10), depth assert at 230:5 (new), containment assert at
  251:5 (was 223:5), no-acquisition assert at 261:5 (new). The round-3
  record's 198:10 / 223:5 quotes are historical.
* The acknowledged frontier of any string-anchored pin remains the
  split-across-lines anchor game (`let graph =\n{`), which
  `strip_rust_shell` cannot help with and which the round-3 review already
  named; the depth check is itself string-anchored, so it shares that
  frontier (an anchor split across lines or spelled with different
  whitespace is not found, which fails the pin's expects closed — it cannot
  create a false pass). (The round-5 review falsified the "cannot create a
  false pass" half for the no-acquisition `contains` — mutation (n2), a
  spaced `memory.graph().read ()` after the block's close, passed the pin
  with the guard held across the build — and the round-5 remediation closes
  the whole class for the build pin: it flattens every ASCII whitespace run
  out of the stripped body before anchoring, so a split or whitespace-spelled
  anchor is now *found* by the flattened text, not missed, and the
  no-acquisition window is matched on the same flattened text. The
  figures-first pin is unchanged and still anchors on the raw body.)
* The milestone itself is unchanged: `view.rs` is byte-identical to the
  round-1/round-2/round-3 hash, `of_memory` holds the guard for exactly one
  copy, and every pin and gate is green. This finding was about the review
  pin's soundness, not the code.
* The tree stays dirty with the implementation + all four remediations +
  this record, exactly as the orchestrator expects; nothing committed.
