# M12a round-5 remediation

Remediates the single finding in `m12a-round5.md` — P2 R5-1: the round-4
pin's SAFETY comment claimed the Linux umask is per-thread; it is not, and
the round-4 shape (setting the umask in a suite thread) transiently applied
umask `0` to every thread of the test binary. No deferrals. Base and
destination: branch `main`; the tree is left dirty for the orchestrator,
nothing committed.

## The finding, restated

The round-4 pin set the calling thread's umask to `0o000` around the
sandbox create/write, restoring it in a `Drop` guard. The SAFETY comment
claimed "On Linux the umask is per-thread and does not affect other tests;
on macOS it is process-wide". That Linux claim is **false**: the umask
lives in `struct fs_struct`, which ordinary threads share via `CLONE_FS`, so
`umask()` in one thread changes the effective umask of every thread in the
process. Linux 4.7 added only the `/proc/<pid>/status:Umask` observability
field, never per-thread semantics. The round-5 reviewer proved it three
independent ways on this machine (sibling-thread file created at `0666`
during the window; `/proc/self/status` showing a child's `umask()` change
propagate `22 -> 77`; a standalone C/pthreads program reproducing `0666`).

The pin itself bit deterministically and the `Drop` guard restored on panic
— both verified — but the shape was wrong: during the window, every
concurrently running suite thread created files at umask 0. No shipped
assert trips today (the mode-asserting tests are umask-independent — the
reviewer ran a 50 ms widened window against the ops pin and it stayed
green), but the justification was false and the window is a latent hazard
for any future file-creating, mode-asserting test.

## The fix

The create/write/stat now run in a **forked child** under umask `0`, and the
two modes come back to the parent over a pipe. The child sets its own umask
(a forked process has its own `fs_struct` copy), so no suite thread's umask
is ever touched — the process-wide race window is gone by construction, and
the comment can state the truth without caveats:

* the child: `umask(0o000)` → `Sandbox::create()` → `write_script()` →
  stat both → write the two modes as 8 little-endian bytes → `_exit(0)`
  (or `_exit(2)`/`_exit(3)` on failure, which the parent reports);
* the parent: closes the write end, `waitpid`s the child and checks
  `WIFEXITED`/`WEXITSTATUS`, reads the 8 bytes, and asserts `0o700`/`0o600`.

The child avoids panicking and unwinding entirely — `?`-propagated errors
become a non-zero exit, and `_exit` runs no destructors, so nothing from the
harness runs in the child. The `pthread_atfork` note is in the `fork`
SAFETY comment: glibc reinitialises the allocator locks in the child, so the
allocations `Sandbox::create`/`write_script` make are safe even though the
test binary is multi-threaded.

The `set_permissions` fix itself is untouched; only the pin's shape changed.

## Mutations

Every mutation transient; the file restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each run.
Both mutations run against the fork shape, so the child reports the wide
modes and the parent's asserts fire:

| Mutation | Pin | Result |
| --- | --- | --- |
| dir mode-setting dropped from `Sandbox::create` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox dir must not be readable by other accounts, got 777`, `left: 511, right: 448` (`0o777` vs `0o700`) |
| script mode-setting dropped from `write_script` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox script must not be readable by other accounts, got 666`, `left: 438, right: 384` (`0o666` vs `0o600`) |

The quoted outputs are byte-identical to round 4's, so the pin's property
is unchanged by the shape change.

**Determinism, proven against the configuration that defeated the round-3
pin:** the pin passes with the ambient umask at `0o022` (ordinary) and at
`0o077` (the hardened value under which the round-3 pin was blind) — the
child's own umask `0` makes the ambient value irrelevant.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **538 lib passed, 0 failed, 2 ignored** (540 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`report_pin`).
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `scratch.rs` 798 lines, under 1000.

## What was executed vs. only read

**Executed.** Both mutations against the fork shape, each reverted and
hash-verified. The pin on the fixed tree at ambient umask `022` and `077`.
The full suite, clippy, fmt.

**Read, not executed.** The reviewer's three proofs of the process-wide
Linux umask (sibling-thread `0666`, `/proc/self/status` propagation, the
standalone C program) — accepted as the finding's evidence; the fix makes
the question moot. No non-unix build is available; the test is
`#[cfg(unix)]` and the fork is inside it.
