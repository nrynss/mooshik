# M12b round-6 remediation

Remediates the two findings in `m12b-round6.md` — P2 M12b-R6-1 (nine
compiling spellings of the guard acquisition — receiver respelled with parens,
hoisted, UFCS on the method, a binding alias, UFCS on the lock, `try_read`,
deref, block receiver, and a module-level helper call — each bound a
function-scope `parking_lot::RwLockReadGuard` after the copy block's close and
held it across the whole `of_graph` build, and the text-anchored pin passed
every one) and P3 M12b-R6-2 (the no-macro `!(`-window also rejected legitimate
`!(…)` unary-not expressions — a dormant false positive). No deferrals. The
reviewer's convergence judgment is implemented: the text-anchored pin is
replaced by a token-level (syn) pin; the acquisition is defined structurally.
Base and destination: branch `main` at `709e911`; the tree is left dirty for
the orchestrator, nothing committed. All mutations below were transient:
`src/memory/view.rs` restored from a byte copy and `sha256sum`-verified
identical to the pre-mutation state after every run
(`c30de8258879f65064e340497aff7cf7c0a3eba87f143dae65754a3951e59031`, the
same hash every prior round recorded — the round-6 remediation changed
`view.rs` not at all). Ambient shell exports a live `LAMBO_POSTGRES_DSN`, so
every `cargo` invocation ran under `env -u` for it, `MOOSHIK_POSTGRES_DSN` and
`DATABASE_URL`.

## The design choice: the review's simpler alternative, made concrete

The reviewer's prescription offered two structural designs: the full
receiver-resolution definition of the acquisition, or the simpler and complete
alternative — **confinement**: no expression referencing the `memory`
parameter may appear outside the copy's block, except a statically-whitelisted
pre-block call. This pin uses **both**, for different jobs:

* **The confinement rule is the gate that makes all nine round-6 forms fail.**
  Every acquisition outside the copy block references `memory`, however it is
  spelled — `(memory.graph()).read()`, `Memory::graph(&memory).read()`,
  `let g = &memory; g.graph().read()`, `parking_lot::RwLock::read(memory.graph())`,
  `memory.graph().try_read().unwrap()`, `(*memory).graph().read()`,
  `{ memory }.graph().read()`, and the helper call `read_lock(&memory)` all
  carry the `memory` identifier (or a binding of it) in an expression outside
  the block, so confinement catches the whole receiver-respelling and
  indirection family at the token level — no string anchor is needed, and
  parens, whitespace, line breaks and comments are gone as disguise because
  the body is parsed, not matched.
* **The reviewer's structural acquisition definition does the
  exactly-one-acquisition check** (point 4 of the prescription): an
  acquisition is a read-family method call (`read`, `read_recursive`,
  `try_read`, `try_read_recursive`) on the memory graph's lock, or a
  read-family call taking the graph as its first argument, with the receiver
  resolved by unwrapping parens, single-expression blocks, references, derefs
  and casts and a one-pass alias map (`let g = &memory;` collected over the
  body), and the UFCS spellings (`Memory::graph(&memory)`,
  `parking_lot::RwLock::read(memory.graph())`) resolved the same way.

The rule, stated precisely (and now the pin's doc comment): the body slice is
parsed as one `syn::ItemFn`; (1) no `Expr::Macro` anywhere in the fn; (2) the
copy is the unique top-level `let graph = { … }` statement, the whitelisted
`let stats = memory.stats();` precedes it, and a top-level `of_graph(…)`
statement follows it; (3) no expression outside the copy's block references
`memory` (confinement); (4) exactly one graph-guard acquisition exists, inside
the copy's block — so the guard's binding scope is the block, and the block
closes before the build statement.

The m18 helper class deserves the note the review demanded: a helper call
*inside* `of_memory`'s body passes `&memory` (or the receiver), so the
confinement catches it — executed, `read_lock(&memory)` after the close fails
at 294:9 exactly like the direct spellings. The class genuinely beyond even a
token pin is indirection that moves the acquisition **out of** `of_memory`'s
body entirely — a helper that returns the guard, or takes the copy with it —
because the body then contains no acquisition tokens at all. That is a
structural change to *where* the guard is taken, not a respelling of the same
code, so it sits outside the pin's named fault, and the R2-2 precedent for a
documented limit applies there (exactly as the module-level `macro_rules!`
did for the text pin); the doc names it. By the same fail-closed stance as the
macro rejection, an in-block helper call that could hide an acquisition is
refused by the count check (executed: probe (p1), `let guard = read_lock(
&memory);` inside the copy block fails the count with `left: 0, right: 1`).

## M12b-R6-1 — the pin is now token-level: the nine round-6 forms fail, and every historical evasion fails with them

The text pin anchored strings on the stripped, whitespace-flattened body: the
no-macro `!(`-window, the `memory.graph().read()` spine, the `letgraph={` open,
the brace-depth close, containment and the no-acquisition `contains`. Six
rounds of the same game — each fix closed one textual class, the next round
re-expressed it through text the new anchors could not see — and this round
found nine survivors at once in the unbounded receiver-respelling and
indirection class. The replacement parses the same slice and judges the AST:

```rust
    let source = include_str!("view.rs");
    let slice = source
        .split("pub fn of_memory")
        .nth(1)
        .expect("of_memory is defined")
        .split("fn of_graph")
        .next()
        .expect("of_graph follows it");
    let close = body_close(slice);
    let item = if close < slice.len() { &slice[..=close] } else { slice };
    let of_memory: syn::ItemFn = syn::parse_str(&format!("pub fn of_memory{item}"))
        .expect("of_memory's body slice parses as a function item: the pin judges \
                 the AST, and cannot judge source it cannot parse");
```

One correction to the review's prescription, found by implementing it: "the
slice is exactly one item" is not quite true of the current file — of_graph's
doc comment sits between of_memory's own close and `fn of_graph`, inside the
slice. The pin truncates the slice at the fn body's closing brace first
(`body_close`, a brace-depth scan with comments and literals skipped, so a
brace inside a string or comment cannot close the body early), so the slice
parses as one item. The checks then run in order:

| Check | Site | Proves |
| --- | --- | --- |
| No macro invocation anywhere in the fn (`Expr::Macro` walk) | 239:5 | An invocation's expansion is invisible to the AST, so any macro fails closed; a unary `!(x)` is `Expr::Unary`, never a macro |
| The copy is one top-level `let graph = { … }` statement (expect), and exactly one | 249:61, 254:5 | The flat form has no block; a decoy adds a second `let graph = { … }` |
| `let stats = memory.stats();` precedes the copy | 265:5 | Figures first, guard second — the recursion-safety order |
| A top-level `of_graph(…)` statement follows the copy | 270:57, 274:5 | The build runs after the block closes, so the guard dropped at the close is not held across the build |
| **Confinement**: no expression outside the copy's block references `memory` | 294:9 | Every acquisition outside the block — however spelled — references `memory`; this is the gate that fails the round-6 nine and the hoisted/decoys/nested/unsafe/spaced/multiline classes |
| Exactly one graph-guard acquisition (structural definition above), inside the copy's block | 312:5, 324:5 | A second acquisition, or an in-block helper call that could hide one, fails the count; the one acquisition's binding scope is the block |

The 274:5 and 324:5 asserts are belt-and-braces: 324's premise is implied by
confinement (an acquisition references memory, and memory is confined to the
block), and 274 cannot be reached by compiling code (any `of_graph` call needs
the copy, which the copy-expect pins before it) — both are kept as the doc's
intent made executable.

## M12b-R6-2 — the unary-not false positive is closed as a side effect

The text pin's no-macro check matched `!` immediately followed by `(`, `{` or
`[` on the flattened body, so a legitimate `let _b = !(1 > 2);` was rejected
with the macro message. The structural pin rejects `Expr::Macro` in the AST,
and a unary not parses as `Expr::Unary` — executed: mutation (m17),
`let _b = !(1 > 2);` inside the body, **passes** (`ok. 1 passed`), the only
change from the old pin's behavior on that form. The macro class stays closed
at the token level: module-level `macro_rules! grab` + `grab!(memory)` after
the close and `grab!(memory)` inside the block both fail at 239:5, verbatim
class (m1)/(o2) below.

## The proof

Every mutation transient; `src/memory/view.rs` restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each
(`c30de825…`). The pin run on the shipped bytes before the mutations and again
after the final revert. All runs in a clean env (the three DSN variables
unset). 36 battery runs + 5 extra probes = 41 executed forms.

| Mutation | Pin | Result |
| --- | --- | --- |
| (a) shipped block-scoped form | `the_build_runs_against_the_copy_and_not_the_guard` | **passes** — twice: baseline before the mutations and after the final revert, `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out` |
| (b) flat three-statement form (round-1 mutation (c)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:249:61: the copy is a block: of_memory must copy the graph out from under the guard inside a `let graph = { … }` statement, so the guard's binding scope ends at the block's close`; zero warnings |
| (c) guard binding hoisted above the copy block, block retained | same | **caught** — `panicked at src/memory/view_session_tests.rs:294:9: no expression outside the copy's block may reference the `memory` parameter: the guard is taken only inside the block, so a memory reference after its close is an acquisition (or the receiver, alias or argument of one) bound at function scope and held across the build`; zero warnings |
| (d) decoy block `let graph = { 0 };` before a hoisted guard | same | **caught** — two `let graph = { … }` statements: `panicked at src/memory/view_session_tests.rs:254:5: assertion `left == right` failed: exactly one `let graph = { … }` statement may appear at of_memory's top level: the copy is anchored by structure, so a decoy block cannot be the block the guard drops inside`, left: 2, right: 1; one `unused variable: graph` warning |
| (e) line comment carrying `let graph = {` before a hoisted guard | same | **caught** — same 294:9 confinement message, zero warnings (comments never reach the AST) |
| (f1) block comment carrying the anchor before a hoisted guard | same | **caught** — same 294:9 message, zero warnings |
| (f2) string-brace decoy before a hoisted guard | same | **caught** — same 294:9 message, zero warnings (the string is a literal token, not a block) |
| (f3) raw-string anchor `r#"let graph = { 0 };"#` before a hoisted guard | same | **caught** — same 294:9 message, zero warnings |
| (g) renamed binding hoisted (`let g = …`, copy through `&g`) | same | **caught** — same 294:9 message, zero warnings |
| (h1) closure carrying a complete guard+copy before a hoisted guard | same | **caught** — same 294:9 message, zero warnings (the closure's body references memory outside the copy's block) |
| (h2) nested fn carrying a complete guard+copy before a hoisted guard | same | **caught** — same 294:9 message, zero warnings |
| (h3) match arm carrying a complete guard+copy before a hoisted guard | same | **caught** — same 294:9 message, zero warnings |
| (k) top-level decoy carrying its own guard+copy before a hoisted guard | same | **caught** — same 254:5 uniqueness message, left: 2, right: 1, zero warnings (the decoy is itself a `let graph = { … }`) |
| (m2) `#[cfg(any())]`-gated block carrying a guard+copy before a hoisted guard | same | **caught** — same 254:5 uniqueness message, left: 2, right: 1, zero warnings (syn parses the cfg'd `let`, so it counts — fail-closed) |
| (n1) module-level `macro_rules! grab` + `let guard = grab!(memory);` after the block | same | **caught** — `panicked at src/memory/view_session_tests.rs:239:5: no macro invocation may appear in of_memory's body: an invocation's expansion is invisible to this AST pin, so it could acquire the graph guard at function scope and hold it across the build`; one `unused variable: guard` warning (M12b-R5-1 stays closed) |
| (o2) `let inner = grab!(memory);` inside the copy block | same | **caught** — macro tokens anywhere fail closed, even one expanding to an in-block acquisition: same 239:5 message, zero warnings |
| (n2) spaced acquisition `memory.graph().read ();` after the close | same | **caught** — tokens parse identically: same 294:9 message; one warning (M12b-R5-2 stays closed) |
| (o3) `memory\n.graph()\n.read();` after the close | same | **caught** — same 294:9 message; one warning |
| (m16) linebreak inside the call parens, `read\n();` after the close | same | **caught** — same 294:9 message; one warning |
| (o4) `let guard = unsafe { memory.graph().read() };` after the close | same | **caught** — same 294:9 message; two warnings (`unnecessary \`unsafe\` block`, `unused variable: guard`) |
| (iv-iflet) guard acquired in an `if let` scrutinee after the close | same | **caught** — same 294:9 message, zero warnings |
| **(m8) `let guard = (memory.graph()).read();` after the block — the documented parens-receiver frontier** | same | **caught** — `panicked at src/memory/view_session_tests.rs:294:9: no expression outside the copy's block may reference the `memory` parameter: …`; one warning (P2 M12b-R6-1 closed) |
| **(m8b) `(memory.graph()).read();` hoisted above the block** | same | **caught** — same 294:9 message, zero warnings |
| **(m9) `let guard = Memory::graph(&memory).read();` after the block (UFCS on the `&self` method)** | same | **caught** — same 294:9 message; one warning |
| **(m10) `let g = &memory; let guard = g.graph().read();` after the block (binding alias)** | same | **caught** — same 294:9 message; one warning (the `&memory` binding outside the block is itself the reference) |
| **(m12) `let guard = parking_lot::RwLock::read(memory.graph());` after the block (UFCS on the lock)** | same | **caught** — same 294:9 message; one warning |
| **(m13) `let guard = memory.graph().try_read().unwrap();` after the block (read-family name)** | same | **caught** — same 294:9 message; one warning |
| **(m14) `let guard = (*memory).graph().read();` after the block (deref receiver)** | same | **caught** — same 294:9 message; one warning |
| **(m15) `let guard = { memory }.graph().read();` after the block (block receiver)** | same | **caught** — same 294:9 message; one warning |
| **(m18) module-level `fn read_lock(m: &Memory) -> parking_lot::RwLockReadGuard<'_, Graph>` + `let guard = read_lock(&memory);` after the block** | same | **caught** — same 294:9 message; one warning (the `&memory` argument is a memory reference outside the block) |
| (f4) safe string-brace: string `{` inside the copy block, guard correctly bound | same | **passes** — `ok. 1 passed`, zero warnings; no false positive |
| (i1/l) safe nested guard scope inside the copy block (the copy wrapped in one extra `{ }` layer) | same | **passes** — `ok. 1 passed`, zero warnings; the acquisition still sits inside the copy's block |
| **(m17) `let _b = !(1 > 2);` inside the body (legitimate unary not)** | same | **passes** — `ok. 1 passed`, zero warnings (P3 M12b-R6-2 closed: `Expr::Unary`, never `Expr::Macro`) |
| (m1) safe `#[allow(unused_variables)]` attribute on the copy statement | same | **passes** — `ok. 1 passed`, zero warnings |
| (v-const) safe `const`/`static` items between the body brace and the copy | same | **passes** — `ok. 1 passed`, one `unnecessary braces around assigned value` warning on the mutation's own `{ 1 + 1 }` |

Five further probes, beyond the battery, exercising the remaining checks:

| Mutation | Pin | Result |
| --- | --- | --- |
| (j) guard/copy/build folded into one enclosing block as the body's tail (round-2/round-5 (j)) | same | **caught** — `panicked at src/memory/view_session_tests.rs:249:61: the copy is a block: …`; zero warnings |
| (p1) `let guard = read_lock(&memory);` **inside** the copy block (module helper; safe shape, unverifiable) | same | **caught** — `panicked at src/memory/view_session_tests.rs:312:5: assertion `left == right` failed: exactly one graph-guard acquisition may appear in of_memory's body: a read-family method call (`read`, `read_recursive`, `try_read`, `try_read_recursive`) on the memory graph's lock, or a read-family call taking the graph as its first argument, however the receiver is spelled`, left: 0, right: 1; zero warnings (fail-closed: the count cannot see a helper's expansion) |
| (p2) two acquisitions inside the copy block (`g1`, `g2` both `memory.graph().read()`) | same | **caught** — same 312:5 message, left: 2, right: 1; one warning |
| (p3) `let stats = memory.stats();` moved after the copy | same | **caught** — `panicked at src/memory/view_session_tests.rs:265:5: the figures must be read before the copy: `Memory::stats` takes the graph lock itself, and the read lock is not recursion-safe`; zero warnings |
| (p4) build folded into the copy block (block value returned by a tail `graph`) | same | **caught** — `panicked at src/memory/view_session_tests.rs:270:57: the build follows the copy: the body must end with a top-level `of_graph(…)` statement`; zero warnings |

Each mutation reverted and hash-verified identical (`c30de825…`); the
shipped-form runs came from the restored bytes. The checks fail in order:
macro 239:5, copy expect 249:61, uniqueness 254:5, stats order 265:5, build
expect 270:57, confinement 294:9, count 312:5. All nine round-6 forms fail;
all twenty-one historical evasions (the twenty in the battery plus the (j)
folded probe) fail; all five safe forms (plus the two baselines) pass — the
structural pin catches the whole sequence's classes at once, exactly what the
convergence judgment asked of it.

## Cargo.toml / Cargo.lock

```toml
[dev-dependencies]
# The build pin parses of_memory's body with `syn` instead of matching text:
# the receiver-respelling and indirection class the text pin could not close
# is gone at token level. "full" parses the fn item; "visit" powers the
# structural walks. Reuses the 2.0.x already vendored transitively in the
# lock (serde_derive/schemars build it), so no new compile weight.
syn = { version = "2", features = ["full", "visit"] }
```



`Cargo.lock` changes by exactly one line: the root package's dependency list
gains `syn 2.0.119` — the version already vendored transitively in the lock
(serde_derive/schemars build the same 2.0.x), so no new package or compile
weight enters the tree. `features = ["full"]` per the review's prescription,
plus `"visit"` for the structural walks (`syn::visit::Visit`); features are
not recorded in the lock. `cargo test --locked` resolves and runs (gates
below) — the lock update is part of this deliverable.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset), on the final tree:

* `cargo test --locked` → **544 lib passed, 0 failed, 2 ignored** (the two
  pre-existing live-Cloud/print-only ones) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.01 s) **+ 0 doc**, exit 0 — matches every prior
  record's numbers exactly. Lib phase 14.30 s.
* `cargo clippy --locked --all-targets --all-features` → clean, exit 0 (one
  `needless_lifetimes` on the `unwrap` helper found and fixed during the run).
* `cargo fmt --check` → clean, exit 0.
* File-size cap → clean. `view.rs` 975 (unchanged), `view_session_tests.rs`
  760 (was 530; +230 — the doc reworked to claim exactly what the AST checks
  prove, the four structural checks, and the syn helpers: `body_close`,
  `unwrap`, the resolver and alias-map functions, the statement predicates and
  the three visitors; `strip_rust_shell` and `flatten_whitespace` removed —
  the text pin is gone — and `literal_len` kept for `body_close`),
  `view_tests.rs` 871, `view_clock_tests.rs` 292, `view_tick_tests.rs` 169,
  `tui/mod.rs` 807, `tui/app.rs` 317, `app_tests.rs` 493, `tui_cmd.rs` 119,
  `cli/tests.rs` 811, `PLAN.md` 735, `Cargo.toml` 62 — all under 1000.
* Lambo still pinned at `4c6fc93`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`, from `Cargo.lock`;
  the lock diff touches only the root package's `syn` line).
* The seven milestone pins green on the final tree, individually: `a_rebuild_
  sees_a_write_from_elsewhere_without_a_keystroke`, `a_tick_rebuilds_the_live_
  workspace_and_leaves_the_demo_alone`, `the_figures_are_read_before_the_graph_
  guard`, and the four M12a regression pins (`the_local_database_is_created_
  and_repaired_private`, `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`) — each
  `test result: ok. 1 passed; 0 failed; 545 filtered out`. The figures-first
  pin is untouched; the two pins are deliberately separate checks.

## What was executed vs. only read

**Executed.** Forty-one pin runs: the thirty-four-form battery (baseline
twice, (b)–(m18) evasions and (f4)–(v-const) safe forms, every one reverted
and `sha256sum`-verified byte-identical (`c30de825…`) after every run, with
the pin run before and after) plus five probes ((j) folded tail block, (p1)
in-block helper, (p2) double acquisition, (p3) stats-after-copy, (p4)
build-inside-copy); full panic messages re-captured with line and column for
the representative forms (249:61, 254:5, 239:5, 294:9, 265:5, 270:57, 312:5);
warning counts for every run; the compile of every mutation verified (the
mutations must compile — the whole test file compiles); the `expect()`-vs-
`assert!` brace-escape asymmetry verified (`.expect` takes a raw `&str`, no
format processing, so its message spells `{ … }` in the source while the
`assert!` messages spell `{{ … }}` — both render `{ … }`); the syn 2.0.119 API
shapes confirmed from the vendored source (`Stmt::Expr(Expr, …)` is unboxed,
`LocalInit { eq_token, expr, diverge }`, `UnOp::Deref(Token![*])`); the slice
content verified (of_graph's doc comment sits between of_memory's close and
`fn of_graph`, inside the slice — the reason for `body_close`); the seven
milestone pins individually; the full suite in a clean env; clippy; fmt; the
file-size count; the lambo pin re-confirmed from `Cargo.lock`.

**Read, not executed.** The reversed-order contention and writer-starvation
races themselves (the pin failures are established textually by the battery;
rounds 1–3 already demonstrated the wedged behaviour on this machine and no
lock code changed). The `--demo` pty interplay (round 1 executed it; this
remediation touches no TUI code). The measurement harness in
`view_tick_tests.rs` (untouched; no production code changed).

## Notes for M12c

* Both round-6 findings are closed at their site, structurally. M12b-R6-1:
  the pin now parses of_memory's body with `syn` and asserts the shape —
  no `Expr::Macro`; one top-level `let graph = { … }`; the whitelisted
  `let stats = memory.stats();` before it; a top-level `of_graph(…)` after it;
  **no expression outside the copy's block references `memory`** (the
  confinement gate that fails all nine round-6 forms — receiver parens,
  hoisting, UFCS on the method and on the lock, the alias binding, `try_read`,
  deref and block receivers, and the module-level helper call — plus every
  historical evasion: flat, hoisted, decoys, comment/string/raw anchors,
  nested closure/fn/match, the cfg'd decoy, module-level and in-block macros,
  spaced/multiline acquisitions, the `unsafe`-wrapped acquisition and the
  if-let scrutinee); exactly one graph-guard acquisition, inside the block.
  M12b-R6-2: `Expr::Macro` rejection is more precise than the `!(`-window —
  a unary `!(1 > 2)` parses as `Expr::Unary` and passes (executed).
* The design is the review's simpler alternative made concrete, with the
  reviewer's structural acquisition definition doing the count: confinement
  is the gate that makes the nine forms fail, and the unwrapping + one-pass
  alias map + UFCS resolution gives the exactly-one check its definition.
  The alias map is deliberately scope-blind (errs fail-closed); the count
  check refuses in-block helper calls the same way it refuses macros.
* The honest boundary, named in the pin's doc: indirection that moves the
  acquisition **out of** `of_memory`'s body entirely — a helper returning the
  guard, or taking the copy with it — leaves the body with no acquisition
  tokens at all. That is a change to *where* the guard is taken, outside the
  pin's named fault; the R2-2 precedent for a documented limit applies there,
  as the round-6 review prescribed.
* One prescription correction, recorded here: "the slice is exactly one item"
  needed of_graph's doc comment — which sits between of_memory's close and
  `fn of_graph`, inside the slice — cut away; `body_close` truncates at the
  fn's own body brace (comment- and literal-aware) before parsing.
* The check line numbers moved with the rework: macro 239:5, copy expect
  249:61, uniqueness 254:5, stats order 265:5, build expect 270:57, build
  order 274:5, confinement 294:9, count 312:5, in-block 324:5. The round-5
  record's 218:5 / 245:10 / 267:5 / 288:5 / 300:5 quotes are historical.
* The milestone itself is unchanged: `view.rs` is byte-identical to the
  round-1/round-2/round-3/round-4/round-5 hash (`c30de825…`, 975 lines),
  `of_memory` holds the guard for exactly one copy, and every pin and gate is
  green. Six rounds of findings were about the review pin's coverage, not the
  code; the coverage question is now answered at the token level.
* The tree stays dirty with the implementation + all six remediations +
  Cargo.toml/Cargo.lock + this record, exactly as the orchestrator expects;
  nothing committed.
