# M12b round-2 remediation

Remediates the single finding in `m12b-round2.md` — P2 M12b-R2-1 (the
widened starvation pin still could not see the guard's **scope**: a one-line
hoist of the guard binding above the copy block passed with the
`RwLockReadGuard` held across the entire build). No deferrals. Base and
destination: branch `main` at `709e911`; the tree is left dirty for the
orchestrator, nothing committed. Both mutations below were transient:
`src/memory/view.rs` restored from a byte copy and `sha256sum`-verified
identical to the pre-mutation state after each run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the same
hash the round-1 remediation and the round-2 review both recorded).

## M12b-R2-1 — the pin now requires the copy's block to open before the guard

### What was wrong

The round-1 fix made the pin require the copy's block to close before the
build call, which caught the flat three-statement form. It still could not
see where the guard is **bound**. The hoisted form —

```rust
let stats = memory.stats();
let guard = memory.graph().read();
let graph = {
    ViewData::from_graph(&guard)
};
of_graph(&stats, &graph, now)
```

— satisfies the order assert (`guard < copy < build`) and the block-close
check (`body[copy..build]` contains the block's `}`) while `guard` is bound
at function scope and dropped only at the end of `of_memory`, after
`of_graph` returns: the writer starvation at a 250 ms tick the copy exists
to prevent, reintroduced by moving one line up two lines. The round-2
reviewer executed it: the pin passed, `ok`. The pin's doc — "that is the
order the code must be written in for the guard to be gone by the time the
build runs" — was false for that form, and the round-1 remediation record's
"true of the shipped shape and of nothing the pin accepts" repeated the same
overclaim.

### The fix

The pin now also requires the copy's block to open before the guard is
taken, in the same string-anchored style, after the existing order assert
and block-close check:

```rust
    // The block close alone would let the hoisted form — guard bound at
    // function scope above the copy's block — keep the guard alive across
    // the whole build, so the copy's block must open before the guard.
    let block = body
        .find("let graph = {")
        .expect("the copy is a block, opened before the guard");
    assert!(
        block < guard,
        "the guard must be taken inside the copy's block so it drops before the build"
    );
```

The order assert still runs first (the inline-in-call form keeps failing with
the round-1 verbatim starvation message), then the block-close expect (the
flat form keeps failing there), then the block-open assert. The doc comment
now claims exactly what the checks prove and nothing more: "the copy's block
opens before the guard is taken, so the guard is bound inside the block and
dropped when it closes, and the block closes before the build call, so the
guard is gone before the build runs" — with the flat form named as what the
block-close check bites on and the hoisted form as what the block-open check
bites on. The check now enforces the shipped discipline end to end: guard
inside the copy's block, block closed before the build.

### The proof

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). The pin run on the shipped bytes after the hoisted revert and
again after the final revert.

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: after the hoisted revert and after the flat revert, `test memory::view::session_tests::the_build_runs_against_the_copy_and_not_the_guard ... ok`; `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) guard binding hoisted above the copy block, block retained (round-2 mutation X) | same | **caught** — `panicked at src/memory/view_session_tests.rs:198:5: the guard must be taken inside the copy's block so it drops before the build`; `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 545 filtered out` |
| (c) flat three-statement form (round-1 mutation (c)) | same | **caught, round-1 verbatim message kept** — `panicked at src/memory/view_session_tests.rs:191:10: the copy's block closes before the build`; `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 545 filtered out` |

The pin is now scope-aware: it fails on the hoisted form (guard bound at
function scope, block opened after it) and on the flat form (no block to
close), and passes on the shipped shape where the block opens before the
guard and closes before the build. The "only" in that claim did not survive
the round-3 review: a decoy `let graph = { 0 };` before a hoisted guard and
a line comment carrying `let graph = {` both passed the round-2 checks with
the guard alive across the build — M12b-R3-1 — closed by the round-3
remediation, which anchors on comment- and literal-stripped text and
requires the guard between the copy block's own opening brace and its
matching close.

## Round-1 remediation record corrected

`m12b-remediation-round1.md` claimed the widened check was "true of the
shipped shape and of nothing the pin accepts". The round-2 review falsified
that with the hoisted form; the record's sentence now reads "which is true
of the shipped shape and names the flat form as what the missing block close
bites on", with a parenthetical noting that the block close alone did not
yet cover every shape the pin accepted (M12b-R2-1), closed by this
remediation.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`) **+ 0 doc**, exit 0 — matches rounds 1 and 2
  exactly. Lib phase 14.35 s; report_pin 30.02 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  321 (was 308; +3 doc lines, +10 check lines),
  `m12b-remediation-round1.md` 132 — all under 1000.

## What was executed vs. only read

**Executed.** Both mutations against the widened pin (hoisted form fails at
the new block-open assert with the message above; flat form fails at the
block-close expect with the round-1 verbatim message), each reverted and
`sha256sum`-verified byte-identical (`c30de825…`). The pin green on the
shipped form, after the first revert and again after the final revert. The
full suite in a clean env, clippy, fmt, file-size count.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by mutations
(b) and (c); rounds 1 and 2 already demonstrated the wedged behaviour on
this machine and no lock code changed). The inline-in-call mutation (round-1
mutation (b)) — untouched by this fix; the order assert still runs first and
the round-2 review already re-verified it verbatim.
