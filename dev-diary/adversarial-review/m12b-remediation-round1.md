# M12b round-1 remediation

Remediates the two findings in `m12b-round1.md` — P2 M12b-R1-1 (the
starvation pin could not see the guard's scope) and P3 M12b-R1-2 (the doc's
"verifies the records' 6.5 ms / 18.2 ms" sentence does not reproduce at the 4k
shape). No deferrals. Base and destination: branch `main` at `709e911`; the
tree is left dirty for the orchestrator, nothing committed. Both mutations
below were transient: `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`).

## M12b-R1-1 — the pin now requires the copy's block to close before the build

### What was wrong

`the_build_runs_against_the_copy_and_not_the_guard` proved only the textual
order guard < copy < build (source positions). The flat three-statement form —
`let guard = memory.graph().read(); let graph = ViewData::from_graph(&guard);
of_graph(&stats, &graph, now)` — satisfies that order while the
`RwLockReadGuard` stays held for the entire build: Rust drops a `Drop` value
at the end of its scope, never at its last use, so the writer starvation the
copy exists to prevent is re-introduced by the natural simplification, and the
round-1 reviewer watched it pass the pin three runs out of three. The pin's
doc — "that is the order the code must be written in for the guard to be gone
by the time the build runs" — was false for that form.

### The fix

The pin now also requires the copy's block to close between the copy and the
build call — the block-scoped shape the code actually uses contains the `}`,
the flat form does not:

```rust
    // Source order alone would let the flat form — guard, copy, build, no
    // block — keep the guard alive across the whole build, so the copy's
    // block must close between the copy and the build call.
    body[copy..build]
        .find('}')
        .expect("the copy's block closes before the build");
```

The order assert is untouched and still runs first, so the inline-in-call form
(mutation (b)) still fails with the round-1 verbatim message. The doc comment
now reads "guard, then copy, then build — with the copy's block closed before
the build call", which is true of the shipped shape and names the flat form as
what the missing block close bites on. (The block close alone did not yet cover
every shape the pin accepted: round 2 found the hoisted-guard form passing it —
M12b-R2-1 — closed by the round-2 remediation.)

### The proof

| Mutation | Pin | Result |
| --- | --- | --- |
| flat three-statement form (round-1 mutation (c)) | `the_build_runs_against_the_copy_and_not_the_guard` | **caught** — `panicked at src/memory/view_session_tests.rs:188:10: the copy's block closes before the build`; `test result: FAILED. 0 passed; 1 failed; 0 ignored` |
| inline-in-call form (round-1 mutation (b)) | same | **caught, verbatim vs the round-1 record** — `of_memory must copy the graph out under the guard and build from the copy: the current order holds the guard across the build, which starves a writer at a 250 ms tick` |

Each mutation reverted and `sha256sum`-verified byte-identical
(`c30de825…`). On the shipped block-scoped form the pin passes:
`test memory::view::session_tests::the_build_runs_against_the_copy_and_not_the_guard ... ok`;
`test result: ok. 1 passed; 0 failed; 0 ignored`.

## M12b-R1-2 — the doc now compares like with like

### What was wrong

`of_memory`'s doc claimed the measurement "verifies the records' 6.5 ms /
18.2 ms for the build itself, plus the copy". The 1k leg reproduces (build-only
mean 6.6 ms vs the record's 6.5), but the 4k leg does not — build-only is
~24-30 ms against the record's 18.2, because M12b folds the `Derives`
in-neighbour collection into the pass, so the code is no longer the same build
the M12a records timed. The doc's headline numbers (~8 ms / ~29 ms whole, ~9×
margin) are accurate; only the internal comparison sentence was wrong.

### The fix

The paragraph now compares the milestone's whole rebuild against the records'
pre-copy build, and separates out the copy:

> timed with the copy this function adds: the M12a records measured
> 6.5 ms / 18.2 ms for the pre-copy build; this milestone's build, with the
> `Derives` map folded into the same pass, measures ~8 ms / ~29 ms whole at
> the same shapes, of which the copy is ~0.6 ms / ~3.5 ms. The 250 ms tick
> holds with ~9× margin in debug at the larger shape; the release build
> `mooshik` ships measures ~0.9 ms / ~2.9 ms.

No code or test changed.

### The proof

The harness (`memory::view::tick_tests::a_rebuild_fits_the_tick_budget_on_a_session_sized_graph`)
re-run twice in debug, unembedded legs (sums over 3 samples, means divided):

| shape | run | copy | build | whole mean |
| --- | --- | --- | --- | --- |
| 1k | 1 | 1.583 ms / 3 → 0.53 ms | 19.902 ms / 3 → 6.63 ms | 7.16 ms |
| 1k | 2 | 1.602 ms / 3 → 0.53 ms | 19.801 ms / 3 → 6.60 ms | 7.13 ms |
| 4k | 1 | 12.895 ms / 3 → 4.30 ms | 89.874 ms / 3 → 29.96 ms | 34.26 ms |
| 4k | 2 | 10.933 ms / 3 → 3.64 ms | 73.049 ms / 3 → 24.35 ms | 27.99 ms |

The copy means bracket the sentence's ~0.6 ms / ~3.5 ms (0.53 both 1k runs;
3.64 clean / 4.30 loaded at 4k); the whole means bracket the unchanged
~8 ms / ~29 ms headline (7.13-7.16 / 27.99-34.26). The records' 6.5 / 18.2
are now cited as the M12a pre-copy build, which is what they measured.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`) **+ 0 doc**, exit 0 — matches round 1's numbers
  exactly.
* `cargo clippy --all-targets --all-features` → clean, exit 0.
* `cargo fmt --check` → clean.
* File-size cap → clean. `view.rs` 975 (was 974; the doc paragraph gained one
  line), `view_session_tests.rs` 308 (was 299; +3 doc lines, +6 check lines),
  `view_tick_tests.rs` 169 — all under 1000.

## What was executed vs. only read

**Executed.** Both mutations against the widened pin (flat form fails at the
new block-close expect; inline-in-call form fails with the round-1 verbatim
order message), each reverted and hash-verified. The pin green on the shipped
form, before the mutations and again after the revert. The measurement
harness twice in debug (means above). The full suite in a clean env, clippy,
fmt, file-size count.

**Read, not executed.** The round-1 mutation (a) (figures order — untouched
by this fix; its pin and message are unchanged), and the M12a records'
6.5 ms / 18.2 ms numbers themselves, accepted as the finding's baseline for
the reword.
