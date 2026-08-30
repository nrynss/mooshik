# M12a round-4 remediation

Remediates the single finding in `m12a-round4.md` — P3 R4-1: the
sandbox-mode pin `the_scratch_sandbox_and_script_stay_private` was
umask-dependent, so under an ambient umask ≥ `0o077` the reverted fix was
invisible. No deferrals. Base and destination: branch `main`; the tree is left
dirty for the orchestrator, nothing committed. The rounds-2-and-3 changes
remain in the tree, and this round changes only the pin (the `set_permissions`
fix from round 3 is untouched).

## The finding, restated

The round-3 pin asserted the dir at `0o700` and the script at `0o600` from
`fs::metadata` after `Sandbox::create` + `write_script`, with no umask
control. Without the fix, `fs::create_dir` / `fs::File::create` yield
`0o777 & !umask` / `0o666 & !umask`:

* under the ordinary `0o022`: `0755` / `0644` — caught;
* under a hardened `0o077`: `0700` / `0600` — **byte-identical to the fixed
  result**, so the mutation passed and the pin was green on the code it
  exists to guard against.

The round-3 record's claim that the pin fails the revert "under any realistic
umask" was false for every umask ≥ `0o077`. The round-3 reviewer prescribed a
deliberately wide umask (`0o000`-style) or the graph.db widen/re-provision
shape; neither had been delivered.

## The fix

The pin now drives the umask itself, exactly as prescribed. The calling
thread's umask is set to `0o000` before the create/write and restored
afterwards, so the pre-fix code yields its *widest* possible modes
(`0o777`/`0o666`) no matter what umask the suite runs under — a dropped
`set_permissions` is now always visible. The assertions are unchanged.

One refinement over the reviewer's sketch: the restore runs in a `Drop`
guard rather than as a bare line, because a panic inside the create/write
would otherwise leave the umask at `0` for the rest of the suite.

**Superseded by round 5.** This pin set the umask in a suite thread, which
was wrong twice over: the umask lives in the process-shared `fs_struct`
(CLONE_FS), so `umask()` in one thread is **process-wide on Linux too**, not
per-thread — round 5 proved it and replaced the whole shape with a forked
child (see `m12a-remediation-round5.md`), so no suite thread's umask is ever
touched. The mutation outputs below remain the property the pin must hold.

```rust
let _guard = UmaskGuard(unsafe { libc::umask(0o000) });
let sandbox = Sandbox::create().unwrap();
let script = sandbox.write_script("echo hi", ScratchLanguage::Bash).unwrap();
// drop(_guard) — the umask is back before any assert reads a mode
```

## Mutations

Every mutation transient; the file restored from a byte copy and
`sha256sum`-verified identical to the pre-mutation state after each run.

| Mutation | Pin | Result |
| --- | --- | --- |
| dir mode-setting dropped from `Sandbox::create` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox dir must not be readable by other accounts, got 777`, `left: 511, right: 448` (`0o777` vs `0o700`) |
| script mode-setting dropped from `write_script` | `the_scratch_sandbox_and_script_stay_private` | **caught** — `assertion \`left == right\` failed: sandbox script must not be readable by other accounts, got 666`, `left: 438, right: 384` (`0o666` vs `0o600`) |

Both ran under the pin's own umask `0o000`, so they hold under any ambient
umask the suite could run at. The pin passes on the fixed tree (run after the
Drop-guard change).

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset):

* `cargo test --locked` → **538 lib passed, 0 failed, 2 ignored** (540 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`report_pin`).
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `scratch.rs` 708 lines, under 1000.

## What was executed vs. only read

**Executed.** Both mutations above against the final tree, each reverted and
hash-verified. The pin on the fixed tree. The full suite, clippy, fmt.

**Read, not executed.** Nothing remains true to re-verify here — the round-5
review proved the Linux umask is process-wide (shared `fs_struct`, CLONE_FS),
not per-thread, and the fork shape replaced this pin wholesale. No non-unix
build is available; the test is `#[cfg(unix)]` and the fork is inside it.
