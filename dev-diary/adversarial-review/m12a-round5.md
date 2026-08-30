# M12a round 5 — adversarial closing review of the round-4 remediation

Reviewed at HEAD `47d4e91`, branch `main`, tree dirty with the round-2/3/4
remediations (6 modified source files + the five untracked records). Scope:
the R4-1 pin fix (`the_scratch_sandbox_and_script_stay_private` now drives
the calling thread's umask through a Drop-guarded `UmaskGuard`), the record's
honesty, a confirmation sweep that all round-2/3 pins still hold, and the
no-behaviour-change check. All transient edits reverted — after each mutation
`src/tools/scratch.rs` was restored from a byte copy and `sha256sum`-verified
identical to its pre-mutation state (`0f79307f…`), and `git status
--porcelain` shows exactly the same 6 modified + 5 untracked as before I
started, now with this record beside them. Nothing committed.

## Verdict

**REMEDIATE** — 1 × P2.

The R4-1 fix works: both quoted mutations reproduce verbatim from the shipped
assert, the pin bites under any ambient umask (I re-ran the dir-mutation under
`umask 077` — the exact configuration that defeated the R3 pin — and it is
caught with identical output), and the Drop guard demonstrably restores the
umask after a panic. But the fix's safety justification is false: the SAFETY
comment claims the Linux umask is per-thread and "does not affect other
tests". On this machine — and as a matter of Linux semantics generally, not
just this kernel — the umask is **process-wide for ordinary threads**, so the
pin's `umask(0)` window transiently applies to every suite thread. I proved
this three independent ways (Rust sibling-thread file lands at `0666` during
the window; `/proc/self/status` shows a child thread's `umask()` change
propagating to the reader; a C pthreads program reproduces the `0666` outside
the Rust harness), and a web check of the kernel model confirms it: the umask
lives in `struct fs_struct`, shared by `CLONE_FS` — Linux 4.7 only added the
`/proc/<pid>/status:Umask` observability field, it never made the umask
per-thread. The record's claim that the comment "documents both the Linux
per-thread semantics and why the transient macOS process-wide window is
harmless" is false about the platform, and the round-4 finding's own
prescribed remediation asked for "a SAFETY comment noting the process-wide
race window against the suite's other file-creating tests" — the delivered
comment asserts the opposite for Linux.

## What held up under attack

* **R4-1 — the pin is now umask-deterministic, and the guard restores even on
  panic.** `UmaskGuard(libc::mode_t)` (src/tools/scratch.rs:682-691) stores
  the previous umask and restores it in `Drop`; `let _guard = …` (not `let _
  = …`) keeps it alive for the whole test, so the restore runs on the normal
  path, on a panic inside the create/write, and on a failed assert — all
  three are the same unwind. I verified the panic path by execution (a
  guard-identical struct with a panic inside `catch_unwind`; the umask reads
  back at `0o022` after the panic, assert green). The `libc::mode_t` type
  resolves (the crate's libc dependency is shared with tui/mod.rs) and the
  whole test is `#[cfg(unix)]`, so nothing leaks into a non-unix build. The
  `set_permissions` blocks the pin guards — dir `0o700` after `create_dir`
  (:443-448), script `0o600` after `File::create` (:461-466) — are present
  and cfg-unix-gated. Both mutations and the umask-077 variant are in the
  table below; the quoted outputs reproduce the record verbatim.
* **The record's honesty.** Both quoted mutation failures — `got 777`,
  `left: 511, right: 448` and `got 666`, `left: 438, right: 384` — reproduce
  byte-for-byte from the shipped asserts (the message formats
  `"…got {dir_mode:o}"` / `"…got {script_mode:o}"` are in the tree; the
  numeric sides are the mode arithmetic `0o777 & !umask(0)` / `0o666 &
  !umask(0)` under the pin's own umask 0). The one false note is the
  per-thread Linux claim (the finding below).
* **Rounds 2 and 3 still hold.** `the_local_database_is_created_and_repaired_private`
  (R2-1, ops.rs — untouched by R3/R4) passes: fresh-home half (all three of
  `graph.db`/`-wal`/`-shm` present at 0600, WAL `len() > 0`), the widen to
  0644 and re-provision half all green. `two_sandboxes_opened_in_the_same_instant_are_two_directories`
  (R2-2, scratch.rs — R3/R4-touched) passes: `name(instant)` still the pure
  pid/counter/clock format, counter alone separating two names from one
  instant. Both tui pins (R2-4/R3-2) pass: the disposition readback pin now
  names the signal (`"signal {signal} was left with the session's handler
  installed"`), and the leave pin restores the disposition after itself. The
  four doc-only pins stamp-check against the tree: the `tui_cmd.rs` header
  ("Mooshik's own conflict sentence, which names the holder and no override
  or page this product does not ship") matches the shipped conflict sentence
  and tests; `said`'s doc ("two over-approximations … split on the join
  separator (`"; "`)") is exactly the `contents.contains(said) ||
  said.split(JOIN).all(…)` disjunction; the `action_nodes` doc's
  canonicalization-reuse claim is unchanged and lambo is still pinned at
  `4c6fc93` in `Cargo.lock` (`git+…?rev=4c6fc930f206e6b2505305a2c9c6990aef5fbbe8`).
* **No behaviour change beyond the findings.** The whole diff vs HEAD is the
  expected set and nothing else: the side-file claim loop + widened pin
  (R2-1, ops.rs + resolve.rs), the `name(instant)` extraction + pin rewrite
  (R2-2, scratch.rs), the two doc paragraphs in view.rs (R2-5/R2-6), the
  tui_cmd.rs header (R2-3), the signal capture/restore + both pins (R2-4,
  R3-2, tui/mod.rs), and the sandbox mode-setting + the R4-1 pin rewrite
  (R3-1, R4-1, scratch.rs). I read every hunk of every file; nothing else
  moved, and the R4 delta is contained to the pin region (the `set_permissions`
  blocks match the round-3 remediation's quoted code exactly; scratch.rs is
  708 lines, matching the R4 record's number).

## Findings

### P2

**M12a-R5-1 — The R4-1 pin's SAFETY comment claims a Linux per-thread umask
  that does not exist; the `umask(0)` window is process-wide and affects
  every suite thread.**

The comment (src/tools/scratch.rs:670-677) says "On Linux the umask is
per-thread and does not affect other tests; on macOS it is process-wide …".
The Linux claim is false: the umask lives in `struct fs_struct`, which
ordinary threads share via `CLONE_FS`, so `umask()` in one thread changes the
effective umask of every thread in the process — Linux 4.7 added only the
`/proc/<pid>/status:Umask` observability field, not per-thread semantics. I
confirmed by execution, three independent ways: (1) a sibling thread created
during the pin's `umask(0)` window produced a `0666` file (per-thread
inheritance at spawn would give `0644`); (2) `/proc/self/status` read
`Umask: 0022` before and `0077` after a spawned child called `umask(077)` —
the change propagated; (3) a standalone C/pthreads program on this machine
(kernel `7.2.0-1-cachyos`) reproduced the `0666` outside the Rust harness.
So during the pin's window — microseconds normally, but I widened it to 50 ms
transiently and it behaved identically — every concurrently running test
thread creates files at umask 0: world-writable fixtures in `/tmp` for the
duration of the run, and any future test that asserts a mode on a
umask-created file (or a longer window) fails or leaks spuriously. No shipped
assert trips today — I ran the R2-1 ops pin concurrently against the widened
window and it stayed green, because its claim path (`ensure_private_file_at`,
`open_existing_at` + `set_permissions`) is absolute-mode and the sqlite side
files copy the database's `0600`, which no umask can strip; the suite's other
mode-asserting tests all use absolute `set_permissions` — but that immunity is
exactly the accident the comment claims as Linux design. The round-4 finding
prescribed "a SAFETY comment noting the process-wide race window against the
suite's other file-creating tests", and the round-3 reviewer prescribed "fork
the creation under a wide umask"; the delivered comment asserts the opposite
of reality and the record repeats it. The pin itself is correct — it bites
under every ambient umask (verified, including `077`) and the Drop guard
restores on panic (verified) — but the fix's safety rationale is false and the
process-wide mutation is unacknowledged.

*Remediation.* Make the window not exist: fork the create/write/stat into a
child process (`libc::fork` in the test; the child sets `umask(0)`, creates
the sandbox + script, stats both modes, writes them to a pipe, `_exit`s; the
parent asserts `0700`/`0600`). A forked child gets its own copy of the umask,
so no suite thread is affected and the Drop guard becomes unnecessary; this is
the shape the round-3 review already prescribed. At minimum, if the in-process
window stays, the comment and the record must state that the umask is
process-wide on Linux too and that the window races the suite's other
file-creating tests; the current text ("per-thread … does not affect other
tests") is false and must not ship.

## Mutation-tested pins

Every mutation transient; the mutated file restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each run
(baseline `0f79307fbc0c7694791862dd62a1d224a49e769b395ae20d77ebab0704165ef3`).

| Mutation | Pin | Result |
| --- | --- | --- |
| dir mode-setting dropped from `Sandbox::create` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox dir must not be readable by other accounts, got 777`, `left: 511, right: 448`, verbatim vs the record |
| script mode-setting dropped from `write_script` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox script must not be readable by other accounts, got 666`, `left: 438, right: 384`, verbatim vs the record |
| dir mode-setting dropped, suite run under ambient `umask 077` | `the_scratch_sandbox_and_script_stay_private` | **caught** — identical `got 777`, `left: 511, right: 448`; the R3 pin was green on exactly this revert, the R4 pin is not (the fix's central claim, now executed) |
| panic inside the guarded region (transient guard-identical struct + `catch_unwind`) | — | **restored** — umask reads back at `0o022` after the panic; the Drop guard's restore-on-panic claim holds |

The five pins individually on the final tree, all pass: R2-1 ops,
R2-2 scratch, R3-1/R4-1 scratch, and both R2-4/R3-2 tui pins.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset — the ambient shell exports a live `LAMBO_POSTGRES_DSN`,
so every `cargo` invocation ran under `env -u`):

* `cargo test --locked` → **538 lib passed, 0 failed, 2 ignored** (540 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`report_pin`). Matches the R4 record's numbers exactly.
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `tui_cmd.rs` 100, `ops.rs` 472, `resolve.rs` 299,
  `view.rs` 875, `scratch.rs` 708, `tui/mod.rs` 734 — all under 1000;
  `scratch.rs` matches the R4 record's 708.

## What was executed vs. only read

**Executed.** The three mutation rows above (each reverted and hash-verified).
The five pins individually. The full suite in a clean env, clippy, fmt, and
the file-size count. The panic-restore check (transient test, reverted). The
three umask-scope measurements — sibling-thread file at `0666` during the
window, `/proc/self/status` `22 → 77` propagation, and the C/pthreads
reproduction — plus the 50 ms widened-window run of the scratch pin
concurrently with the R2-1 ops pin (both green, proving the ops pin's modes
are umask-independent). The lambo pin re-confirmed from `Cargo.lock`.

**Read, not executed.** The non-unix stubs (type- and call-site-verified; no
non-unix target is available, as in prior rounds). The macOS process-wide
umask semantics themselves (not testable here; the Linux side of the comment
is falsified by execution, which is what the finding rests on). The claim
that the R4 delta touched only the pin region is by reconstruction from the
round-3 record's quoted code and line counts, since no R3 tree snapshot
exists; the current `set_permissions` blocks match the R3 record's quoted
code verbatim.

## Notes for M12b

Standing items, unchanged and re-verified this round: `of_graph` is `pub`
(src/memory/view.rs:191) and the lock-order pin reads only `of_memory`'s body
(src/memory/view_session_tests.rs:128-144); the R2-2 pin tests `name`
directly, not `create`'s wiring to it; `dev-diary/PLAN.md`'s M12b bullet
("M12b — the tick", lines 682-685) still lacks the lease guard-duration item;
the cloned-slice fix remains deferred to M12b. New: the R5-1 remediation
(fork the sandbox creation in the pin, or correct the comment) is a
round-6-or-M12b item if the orchestrator defers — but the milestone's
no-deferral standard makes the fork the closing-gate fix.
