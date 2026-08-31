# M12b round 4 — adversarial re-review of the round-3 remediation

Reviewed at HEAD `709e911`, branch `main`, tree dirty with the M12b implementation
plus the round-1, round-2 and round-3 remediations (10 modified + 7 untracked —
the full expected set, verified identical before and after every mutation).
Scope: the single round-3 finding (P2 M12b-R3-1 — the block-open check was
anchor-foolable by a decoy block or a comment carrying the anchor string) and
the milestone's continuing hold. All transient mutations reverted and
`sha256sum`-verified identical to the pre-mutation state after each
(`src/memory/view.rs`
`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the same
hash every prior round recorded — the round-3 remediation changed `view.rs`
not at all); `git status --porcelain` shows exactly the same 10 modified + 7
untracked as before I started, now with this record beside them. Nothing
committed. Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so every `cargo`
invocation ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and `DATABASE_URL`.

## Verdict

**REMEDIATE** — 1 × P2.

The round-3 fix is genuine against every form it was built for — the nine-form
proof reproduces verbatim (shipped passes twice; flat, hoisted, decoy,
line-comment, block-comment, string-brace, raw-string and renamed-binding
forms all fail with exactly the quoted messages) — and the new stripper closes
the round-3 evasions: escaped-quote strings, `r##` raw strings, byte strings
and char-literal quotes inside comments are all stripped and the mutations
fail at 223:5 with zero warnings. The safe forms do not false-positive: a
guard bound inside a nested scope that closes before the build, and a copy
block carrying nested blocks and a closure, both pass. Lifetimes survive the
stripper as the doc claims. What survives is a *refinement* of the same anchor
class round-3 closed, and it passes the pin with the hazard present:

```rust
// closure-decoy form — passes, guard held across the build
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

The pin's anchors are first-occurrence finds on the stripped body, and its
containment check trusts the first `let graph = {` to be the copy's block.
A nested scope — closure, nested `fn`, match arm — that carries its *own*
complete guard+copy block satisfies every check: the closure's `guard` is
inside the closure's `graph` block, so `open < guard < close` holds, `guard <
copy < build` holds, a `}` sits between the closure's copy and the build, and
the pin passes — while `of_memory`'s real `let guard = memory.graph().read();`
sits at function scope and is dropped only after `of_graph` returns: the
writer starvation at a 250 ms tick the copy exists to prevent, reintroduced by
one never-called closure (or nested fn, or match arm). Three forms executed,
all `ok`, all with the guard alive across the build. The round-3 fix closed
the decoy class only for decoys *without* an internal guard+copy; the round-3
remediation record's claim that the round-4 attack surface "is now closed"
does not survive. By the same precedent that made R2-1 and R3-1 P2s — a pin
passing with the hazard present — this is a P2, and per the round's
instruction there are no deferrals.

## What held up under attack

* **The round-3 nine-form proof reproduces exactly.** Shipped passes twice
  (baseline and final revert); flat → `panicked at
  src/memory/view_session_tests.rs:198:10: the copy's block closes before the
  build` (round-1 verbatim message); hoisted, decoy, line-comment,
  block-comment, string-brace decoy, raw-string anchor and renamed-binding
  hoist → `panicked at src/memory/view_session_tests.rs:223:5: the guard must
  be taken inside the copy's block so it drops before the build` (round-3
  verbatim); safe string-brace → `ok`. Each mutation reverted and
  hash-verified after each run.
* **The stripper handles the round-3 named evasion classes.** Escaped quotes
  inside a string (`"let graph = { 0 }; \" …"`), `r##` raw strings, byte
  strings `b"…"`, and a `'` inside a line comment before a hoisted guard — all
  stripped, all caught at 223:5, zero warnings. The record's claims about the
  stripper hold for the shipped body: its comments and literals are removed
  and the surviving code has no literals, so the anchors land where they did.
* **The safe forms do not false-positive.** (i) A guard bound inside a nested
  scope *inside* the copy block that closes before the build (guard and copy
  both in the nested block) — the guard drops when the nested block closes,
  before the build — **passes**, and correctly so. (ii) A copy block carrying
  a nested `{ let a = 1; a + 1 }` block and a closure before the guard —
  passes; the brace count survives nested braces exactly. (iii) Lifetimes in a
  nested `fn helper<'a>` inside `of_memory` (safe form) — passes; the
  stripper's char handling is line-bounded as the doc claims and the mangling
  it does produce never touches an anchor or brace on the shipped shape.
* **The pin anchors on the acquisition call, not the binding name.** A second
  renamed-binding hoist (`let g = memory.graph().read();`, copy through `&g`)
  fails identically at 223:5.
* **The milestone still holds.** `a_rebuild_sees_a_write_from_elsewhere_without_a_keystroke`
  (real sqlite `Memory`, the write lands in the trickle without a keystroke),
  `a_tick_rebuilds_the_live_workspace_and_leaves_the_demo_alone`,
  `the_figures_are_read_before_the_graph_guard`, and all four M12a regression
  pins (`the_local_database_is_created_and_repaired_private`,
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`) — green
  on the remediated tree, individually run.
* **The round-3 remediation touched only the pin and the round-2 record's
  correction.** `view.rs` is byte-identical to the recorded hash
  (`c30de825…`, 975 lines); `view_session_tests.rs` is 435 lines (was 321;
  +114 — the reworked pin doc, `strip_rust_shell`, `literal_len` and the
  open/close containment checks); `m12b-remediation-round2.md` carries the
  corrected "passes on the shipped shape" sentence with the round-3
  parenthetical. The figure pin's `.split("pub fn of_graph")` → `.split("fn
  of_graph")` change lives in the pin file too; it is benign (the private
  `of_graph` still splits correctly, and the looser split also matches a
  future `pub fn of_graph`). Nothing else in the tree changed between rounds.
* **The R1-3 structural close stands.** `fn of_graph(stats: &MemoryStats,
  graph: &ViewData, now)` is private; `of_memory` is the only production route
  from a `Memory` to a `Workspace`, so the pins cover every caller there can
  be — re-confirmed by grep over `src/`.
* **Gates and pin.** Full suite in a clean env, clippy, fmt, file-size caps,
  lambo pinned at `4c6fc93` — all as recorded below.

## Findings

### P2

**M12b-R4-1 — the containment check is satisfiable by a complete guard+copy
block nested inside a closure, nested fn or match arm: the pin passes with
the guard held across the build.**

The round-3 fix anchors the open/close containment on the *first* `let graph
= {` in the stripped body and requires `open < guard < close` on that block.
That is sound against a *guardless* decoy — a decoy with no internal guard
closes before the hoisted guard, so `guard < close` fails — but the anchor
can be satisfied by a nested scope that contains its own complete guard+copy.
Three executed forms, all compiled clean, all `test result: ok. 1 passed`,
all with `of_memory`'s real guard bound at function scope and alive for the
whole `of_graph` call:

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

In all three, the pin's four checks run against the nested scope's internal
copy: `guard < copy < build` (the closure's acquisition and copy precede the
real build call), a `}` lies between that copy and the build, the first
`let graph = {` is the nested block, and its brace-counted close comes after
its own guard — so `open < guard < close` holds. The pin's doc sentence "the
copy's real block — the first `let graph = {` on the stripped text — opens
before the guard while its matching close comes after it, so the guard
binding sits inside that block and drops when it closes, before the build
runs" is false for all three: the first `let graph = {` is not the copy's
block, and the guard the build actually runs under is the function-scope one
that drops only at the end of `of_memory`. The round-3 remediation record's
claim that the decoy class "is now closed" is true only of decoys without an
internal guard+copy.

*Remediation.* The anchors and braces must come from the function's top
level: the copy's block is a *statement* of `of_memory`'s body, so exactly
one unmatched `{` — `of_memory`'s own body brace — may precede its opening
brace in the stripped text. A nested scope's block sits at depth ≥ 2 (the
closure/fn/match braces precede it), which the depth check rejects, while the
shipped block and every safe nested form (guard in a nested scope, nested
braces inside the copy block) sit at depth 1 and pass. Sketch, inserted after
`let open_brace = …`:

```rust
    // The first `let graph = {` can belong to a nested scope — closure,
    // nested fn, match arm — carrying its own guard+copy, which satisfies
    // every check above while of_memory's real guard is hoisted. The copy
    // block must be a statement of of_memory's body: exactly one unmatched
    // `{` (the body brace itself) may precede it in the stripped text.
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
```

This closes (h1)–(h3) (depths 2, 2 and 3 respectively), keeps the shipped
form and the safe nested forms (depth 1), and leaves the round-3 checks
(guardless decoy, comment and literal anchors, hoisted and flat forms) doing
their existing work. The split-across-lines anchor game (`let graph =\n{`)
remains the acknowledged frontier of any string-anchored pin, as recorded.

## Mutation-tested pins

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`). All
runs under a clean env (the three DSN variables unset).

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:198:10: the copy's block closes before the build`, verbatim vs the round-1/round-3 records |
| (c) guard binding hoisted above the copy block, block retained | same | **caught** — `panicked at src/memory/view_session_tests.rs:223:5: the guard must be taken inside the copy's block so it drops before the build`, verbatim |
| (d) decoy block `let graph = { 0 };` before a hoisted guard | same | **caught** — same 223:5 message; one `unused variable: graph` warning |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — same 223:5 message, zero warnings |
| (f1) block comment `/* let graph = { 0 }; */` before a hoisted guard | same | **caught** — same 223:5 message, zero warnings |
| (f2) string-brace decoy `let graph = { let _string = "{"; };` before a hoisted guard | same | **caught** — same 223:5 message; one `unused variable: graph` warning |
| (f3) raw-string anchor `r#"let graph = { 0 };"#` before a hoisted guard | same | **caught** — same 223:5 message, zero warnings |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok`, 1 passed; no false positive |
| (g) renamed binding hoisted (`let g = memory.graph().read();`, copy through `&g`) | same | **caught** — same 223:5 message; the pin anchors on the acquisition call, not the name |
| (i1) guard+copy inside a nested block *inside* the copy block — guard drops at the nested close, before the build (safe) | same | **passes** — `ok`, 1 passed; the brace count survives nesting and the safe form is accepted, not false-failed |
| (i2) copy block carrying a nested `{ let a = 1; a + 1 }` block and a closure before the guard (safe) | same | **passes** — `ok`, 1 passed; nested braces do not confuse the count |
| (iii1) plain closure decoy `\| \| { let graph = { 1 }; graph }` before a hoisted guard | same | **caught** — same 223:5 message; the decoy's close precedes the guard |
| (iii2) plain match-arm decoy before a hoisted guard | same | **caught** — same 223:5 message |
| **(h1) closure carrying a complete guard+copy before a hoisted guard** | same | **SURVIVES** — `ok`, 1 passed, zero warnings, with the real guard held across the build (P2, M12b-R4-1) |
| **(h2) nested fn carrying a complete guard+copy before a hoisted guard** | same | **SURVIVES** — `ok`, 1 passed, zero warnings, guard held across the build (P2, M12b-R4-1) |
| **(h3) match arm carrying a complete guard+copy before a hoisted guard** | same | **SURVIVES** — `ok`, 1 passed, zero warnings, guard held across the build (P2, M12b-R4-1) |
| (iv1) string with escaped quotes carrying both anchors before a hoisted guard | same | **caught** — same 223:5 message, zero warnings; the whole literal incl. `\"` is stripped |
| (iv2) `r##"…"##` raw string with two hashes before a hoisted guard | same | **caught** — same 223:5 message, zero warnings |
| (iv3) byte string `b"…"` before a hoisted guard | same | **caught** — same 223:5 message, zero warnings |
| (iv4) char-literal quote inside a line comment before a hoisted guard | same | **caught** — same 223:5 message, zero warnings; comments are stripped before literals are considered |
| (iv5) nested `fn helper<'a>` with lifetimes inside `of_memory` (safe form) | same | **passes** — `ok`, 1 passed; a bare lifetime is left alone (line-bounded), the mangling the stripper does produce touches no anchor or brace here |
| (v) second renamed-binding hoist (`let g = …`, copy through `&g`) | same | **caught** — same 223:5 message |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. The first (h3) attempt failed
to compile under my own typing (match arms of incompatible types — the
one-armed `1 => { … }` block valued a `ViewData` against a unit `_` arm); the
fixed unit-armed form compiled clean and is the one recorded above.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.18 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975, `view_session_tests.rs` 435,
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

**Executed.** All twenty-one mutation runs against the pin — (a) shipped
twice, (b) flat, (c) hoisted, (d) decoy, (e) line-comment, (f1) block-comment,
(f2) string-brace decoy, (f3) raw-string, (f4) safe string-brace, (g) renamed
binding, (i1) nested guard scope, (i2) nested braces, (iii1) closure decoy,
(iii2) match-arm decoy, (h1) closure full-copy, (h2) nested-fn full-copy, (h3)
match-arm full-copy, (iv1) escaped-quote string, (iv2) `r##` raw string, (iv3)
byte string, (iv4) quote in comment, (iv5) lifetimes — each reverted and
`sha256sum`-verified byte-identical (`c30de825…`), with the pin run before and
after. The seven milestone pins individually. The full suite in a clean env,
clippy, fmt, file-size count, lambo pin re-confirmed from `Cargo.lock`. The
round-2 record's correction read and confirmed; the figure-pin split change
(`pub fn of_graph` → `fn of_graph`) read and found benign; every hunk of the
full diff read against the current tree; `of_graph`/`of_memory` callers
re-confirmed by grep.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations
(b)–(h3); rounds 1–3 already demonstrated the wedged behaviour on this
machine and no lock code changed). The `--demo` pty interplay (round 1
executed it; this round touches no TUI code). The measurement harness in
`view_tick_tests.rs` (untouched; no production code changed). The depth==1
remediation sketch above is a suggestion in the round-2/round-3 style, not an
executed change.

## Notes for M12c

* The M12b milestone itself is sound and ready to ship — `of_memory` holds
  the guard for exactly one copy, `of_graph` takes `ViewData`, and the
  shipped code is unchanged (`view.rs` byte-identical across all four
  rounds). Every finding in this sequence has been about the *review pin's*
  soundness, not the code.
* The round-3 remediation closed the comment/literal anchor game and the
  guardless-decoy form; the round-4 evasion (a nested scope — closure, nested
  fn, match arm — carrying its own complete guard+copy) needs the
  top-level-statement depth check sketched above. Without it, the pin's doc
  overclaims "the guard binding sits inside that block" exactly as the
  round-3 review found the round-2 doc did.
* `strip_rust_shell`'s char handling is line-bounded as documented, but a
  lifetime followed by another `'` on the same line makes the stripper
  swallow the text between them. That is fail-closed (it can eat an anchor
  and panic the pin; analysis over compilable source shows it cannot create a
  false pass), and the shipped body has no lifetimes — noted for anyone
  extending the scrubber.
* The split-across-lines anchor game (`let graph =\n{`) remains the
  acknowledged frontier of any string-anchored pin, as the round-3 record
  named.
* The tree stays dirty with the implementation + all four remediations +
  this record, exactly as the orchestrator expects; nothing committed.
