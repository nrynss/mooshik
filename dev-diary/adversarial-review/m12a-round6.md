# M12a round 6 — adversarial closing review of the round-5 remediation

Reviewed at HEAD `47d4e91`, branch `main`, tree dirty with the round-2/3/4/5
remediations (6 modified source files + the seven untracked records). Scope:
the R5-1 fork-shape pin (`the_scratch_sandbox_and_script_stay_private` now
forks a child that sets its own umask to `0o000`, creates the sandbox +
script, stats both modes, reports them as 8 LE bytes over a pipe and `_exit`s),
the record's honesty, the round-2/3/4 pins, and the no-behaviour-change check.
All transient mutations reverted — after each, `src/tools/scratch.rs` was
restored from a byte copy and `sha256sum`-verified identical to the
pre-mutation state (`843d3cf1…`), and `git status --porcelain` shows exactly
the same 6 modified + 7 untracked as before I started, now with this record
beside them. Nothing committed.

## Verdict

**APPROVE.**

The round-5 remediation is genuine and complete: the pin is now a forked
child under its own umask, so no suite thread's umask is ever touched and the
process-wide window the round-5 finding attacked is gone by construction. The
fork shape is sound on every axis I attacked it on (fork safety, fd hygiene,
error paths, write atomicity, determinism) — details below. Both shipped-code
mutations reproduce the record's quoted outputs byte-for-byte, the pin passes
at ambient umask `022` *and* `077` (the configuration that defeated the
round-3 pin) plus 30/30 stress runs, and the full suite, clippy, fmt and the
file-size caps are all clean. The record's claims are honest — every quoted
string exists in the tree, the gate numbers match my run exactly, and the
round-4 record's superseded-by-round-5 correction is consistent with both
rounds. Zero findings; no residue from rounds 2-5.

## What held up under attack

* **The fork shape (R5-1).** The child sets `umask(0o000)`, runs
  `Sandbox::create()` + `write_script()`, stats both paths, writes
  `dir_mode` + `script_mode` as 8 little-endian bytes, `_exit(0)`, or
  `_exit(2)`/`_exit(3)` on failure. The parent closes the write end,
  `waitpid`s, checks `WIFEXITED`/`WEXITSTATUS`, then reads the 8 bytes via
  `File::from_raw_fd` + `read_exact` and asserts `0o700`/`0o600`
  (src/tools/scratch.rs:674-797).
  - **Fork safety.** The `pthread_atfork` claim in the SAFETY comment is
    accurate for Linux: glibc's malloc registers atfork handlers that
    reinitialise the allocator locks in the child, and Rust's `System`
    allocator on Linux is malloc, so the allocations `Sandbox::create` /
    `write_script` / `fs::metadata` make after `fork` are safe even though
    the test binary is multi-threaded. The child runs no harness code: the
    closure returns a `Result`, the `match` handles both arms, and every
    exit is `_exit` (which runs no destructors). The one `Drop` that does
    run in the child — `Sandbox`'s `remove_dir_all` of its own directory —
    fires when the closure returns, *before* the report reaches the pipe,
    is a plain filesystem op on the child's own path (no locks, no harness
    state), and is harmless. No panic vector exists on the normal path
    (no `unwrap`/`expect`/`assert` in the child); even a hypothetical
    panic would unwind to the harness and exit non-zero, which the
    parent's `WIFEXITED`/`WEXITSTATUS` asserts catch — the pin can never
    go silently green on a broken child.
  - **fd hygiene.** All four pipe fds close exactly once on every path: the
    child closes the read end (`libc::close(fds[0])`), `_exit` closes the
    child's write end via the kernel; the parent closes the write end
    immediately and the `File` owns the read end, closing it on drop. No
    double-close anywhere (verified by reading every path). The only
    imperfection is a test-failure-only leak: if a parent assert fires
    before `File::from_raw_fd`, `fds[0]` leaks for the process lifetime —
    one fd on an already-failing test, negligible and not a defect of any
    shipped path.
  - **Error paths.** `_exit(2)` (create/write/stat failed) and `_exit(3)`
    (report write failed) are both detected by the parent *before* any
    read: `waitpid` + `WIFEXITED` + `assert_eq!(WEXITSTATUS(status), 0)`.
    I mutation-tested the mechanism: forcing the child's success path to
    `_exit(2)` made the parent fire `assertion \`left == right\` failed:
    the sandbox child could not create or write the sandbox` with
    `left: 2, right: 0`. A truncated report is caught too: mutating the
    child to write only 4 bytes and exit 0 made `read_exact` hit
    `UnexpectedEof` and the pin fail with `the sandbox child wrote no
    report: Error { kind: UnexpectedEof, ... }`. (My first attempt at
    this mutation — changing only the loop bound — *passed*, because the
    single `libc::write` of 8 bytes is atomic and the loop never runs a
    second iteration; that was a malformed mutation, not a pin gap, and it
    incidentally proved the atomicity claim. The corrected mutation, a
    write count of 4, is the one that fails the pin.)
  - **The write loop.** Correct: 8 bytes ≤ `PIPE_BUF`, the pipe is
    blocking with the parent holding the read end open, so the first write
    is atomic and always completes; EINTR is retried; any other error
    exits 3. `written += n` with `n == 0` is unreachable for a positive
    count on a blocking pipe, so no spin.
  - **Determinism.** The pin passes at ambient umask `022` and at `077` —
    the exact hardened value under which the round-3 pin was blind — and
    in 30/30 consecutive runs. Structurally the child's umask is its own:
    `fork` copies `fs_struct` (CLONE_FS), the parent path contains no
    `umask` call at all (`UmaskGuard` is gone from the tree), so no suite
    thread is affected by construction.
* **The record's honesty.** Every quoted string in
  `m12a-remediation-round5.md` exists in the shipped tree: both mutation
  outputs reproduce byte-for-byte from the shipped asserts (`got 777` /
  `left: 511, right: 448` and `got 666` / `left: 438, right: 384` — the
  modes are `0o777 & !0` / `0o666 & !0` under the child's own umask 0),
  the fork-shape description matches the code, and the gate numbers match
  my run exactly. `m12a-remediation-round4.md`'s correction is consistent:
  it states the round-4 shape was wrong twice over (in-process window +
  process-wide umask on Linux) and points at round 5, and its mutation
  table is explicitly retained as "the property the pin must hold" — which
  the fork shape still holds. The SAFETY comment in the tree now states
  the truth ("the umask lives in the process-shared `fs_struct`
  (CLONE_FS), so setting it in a suite thread would apply to every thread
  of the test binary for the window") — the false per-thread claim is
  gone from source and record.
* **Rounds 2-4 still hold.** `the_local_database_is_created_and_repaired_private`
  (R2-1, ops.rs — untouched by R5) passes: fresh-home half (all three of
  `graph.db`/`-wal`/`-shm` present at 0600, WAL `len() > 0`), the widen-to-
  0644-and-repair half all green — at ambient umask `022` and again at
  `077`, confirming the ops pin's mode claims are umask-independent.
  `two_sandboxes_opened_in_the_same_instant_are_two_directories` (R2-2,
  scratch.rs) passes: `name(instant)` is still the pure pid/counter/clock
  format and the counter alone separates two names from one instant. Both
  tui pins (R2-4/R3-2, tui/mod.rs) pass: the disposition-readback pin
  names the signal and leaves both signals at `SIG_DFL` after an
  install/restore pair; the leave pin raises thread-directed (`raise`,
  not `kill`), restores the disposition, and clears the process-wide flag.
  The four doc-only pins stamp-check: the `tui_cmd.rs` header ("Mooshik's
  own conflict sentence, which names the holder and no override or page
  this product does not ship") matches the shipped sentence; `said`'s doc
  ("two over-approximations … split on the join separator (`"; "`)") is
  exactly the `contents.contains(said) || said.split(JOIN).all(…)`
  disjunction in view.rs:464; the `action_nodes` doc's
  canonicalization-reuse claim matches the shipped paragraph; and lambo is
  still pinned at `4c6fc93` in `Cargo.lock`
  (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`).
* **No behaviour change beyond the findings.** I read every hunk of every
  file's diff vs HEAD. The whole change set is exactly the expected one:
  the side-file claim loop + widened pin (R2-1, ops.rs + resolve.rs), the
  `name(instant)` extraction + pin rewrite (R2-2, scratch.rs), the two doc
  paragraphs in view.rs (R2-5/R2-6), the tui_cmd.rs header (R2-3), the
  signal capture/restore + both pins (R2-4, R3-2, tui/mod.rs), and the
  sandbox mode-setting + the pin's evolution through the round-4
  UmaskGuard to the round-5 fork (R3-1, R4-1, R5-1, scratch.rs). Nothing
  else moved; the R5 delta is contained to the pin region of scratch.rs
  (798 lines, matching the R5 record's number).

## Findings

None. The fork shape holds under every attack listed above; the one doc
imprecision that remains — the SAFETY comment names glibc where macOS runs
Apple's libSystem malloc, which registers the same atfork lock resets — is a
platform-specific naming detail with no substance impact (the test is
exercised on Linux, and the property the comment asserts holds on both) and
does not meet the bar for a finding.

## Mutation-tested pins

Every mutation transient; the mutated file restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each run
(baseline `843d3cf16b5a824edf60e2c88e639f1491cbca46d78080a4464c6dc2d1eb48a9`).
Both shipped-code mutations run against the fork shape, so the child reports
the wide modes and the parent's asserts fire:

| Mutation | Pin | Result |
| --- | --- | --- |
| dir mode-setting dropped from `Sandbox::create` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox dir must not be readable by other accounts, got 777`, `left: 511, right: 448`, verbatim vs the record |
| script mode-setting dropped from `write_script` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox script must not be readable by other accounts, got 666`, `left: 438, right: 384`, verbatim vs the record |
| pin plumbing: child's success path forced to `_exit(2)` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: the sandbox child could not create or write the sandbox`, `left: 2, right: 0`; the parent detects both non-zero child exits (2 and 3 share this assert) before reading |
| pin plumbing: child writes only 4 bytes then `_exit(0)` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `the sandbox child wrote no report: Error { kind: UnexpectedEof, message: "failed to fill whole buffer" }`; a partial/empty report cannot pass the parent's `read_exact` |

Determinism, executed: the pin passes with the ambient umask at `0o022`
(ordinary) and `0o077` (the hardened value that defeated the round-3 pin),
and in 30/30 consecutive runs — no flakiness in the fork/pipe/waitpid dance.
The five pins individually on the final tree all pass: R2-1 ops (twice — at
`022` and `077`), R2-2 scratch, R3-1/R4-1/R5-1 scratch, and both R2-4/R3-2
tui pins.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset — the ambient shell exports a live `LAMBO_POSTGRES_DSN`,
so every `cargo` invocation ran under `env -u`):

* `cargo test --locked` → **538 lib passed, 0 failed, 2 ignored** (540 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`tests/report_pin.rs`). Matches the R5 record's numbers exactly.
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `tui_cmd.rs` 100, `ops.rs` 472, `resolve.rs` 299,
  `view.rs` 875, `scratch.rs` 798, `tui/mod.rs` 734 — all under 1000;
  `scratch.rs` matches the R5 record's 798.

## What was executed vs. only read

**Executed.** The two shipped-code mutations (dir, script), each reverted and
hash-verified, and the two pin-plumbing mutations (child exit 2; truncated
4-byte report), each reverted and hash-verified. The pin at ambient umask
`022` and `077`, plus a 30-run stress loop. The R2-1 ops pin at `022` and
`077`; the R2-2 name pin; both tui signal pins. The full suite in a clean
env, clippy, fmt, and the file-size count. The lambo pin re-confirmed from
`Cargo.lock`. Every hunk of the full diff read against the current tree.

**Read, not executed.** The non-unix stubs (type- and call-site-verified; no
non-unix target is available, as in prior rounds). The macOS fork semantics
— the SAFETY comment's glibc naming is Linux-specific, but Apple's malloc
registers the same atfork lock resets, and the substance of the claim (child
allocations are safe) holds on both platforms; the pin is exercised only on
Linux here. The R2-1 WAL-content claim is executed (the assert ran in the
suite), not merely read.

## Notes for M12b

Standing items, unchanged and re-verified this round: `of_graph` is `pub`
(src/memory/view.rs:191) and the lock-order pin reads only `of_memory`'s body
(src/memory/view_session_tests.rs:126-144); the R2-2 pin tests `name`
directly, not `create`'s wiring to it; `dev-diary/PLAN.md`'s M12b bullet
("M12b — the tick") still lacks the lease guard-duration item; the
cloned-slice fix remains deferred to M12b. Nothing new to carry: R5-1 is
closed by the fork shape.
