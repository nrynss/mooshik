# M12a round-3 remediation

Remediates both findings in `m12a-round3.md` — the P2 world-readable
scratch sandbox in `/tmp` and the P3 disposition pin's assert that carried no
message (so the round-2 record's quoted failure was not reproducible from the
shipped code). No deferrals. Base and destination: branch `main`; the tree is
left dirty for the orchestrator, nothing committed. The round-2 remediation's
six modified files remain modified, and this round's two join them.

## Per-finding fixes

### P2 R3-1 — the scratch sandbox was created world-readable in `/tmp`

`Sandbox::create` made its directory with `fs::create_dir` and `write_script`
wrote the model-authored code with `fs::File::create`
(`src/tools/scratch.rs`). Both take the process umask, so under the ordinary
022 the dir came out `0755` and the script `0644` — readable by every account
on the machine from the world-writable temp root, and the `Drop` that removes
them does not run on a kill, so the exposure outlives the run. The interpreter
is exec'd as the invoking user into the sandbox cwd (direct exec, no shell), so
0700/0600 changes nothing functionally.

**Fix.** Both creations now pin their mode with `PermissionsExt::set_mode`,
`#[cfg(unix)]`-gated in the file's existing style; non-unix is untouched:

```rust
fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
    .map_err(|error| fun("tools.scratch_sandbox_failed", &error))?;
```

after `fs::create_dir`, and

```rust
fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
    .map_err(|error| fun("tools.scratch_write_failed", &error))?;
```

after `File::create` in `write_script`. `set_mode` is a `chmod`-equivalent,
not a mode argument, so the umask never touches it. The dir at 0700 blocks
traversal, and the script's own 0600 pins it down regardless.

**Pin.** `the_scratch_sandbox_and_script_stay_private` — creates a real
`Sandbox`, writes a script into it, and reads both modes back over
`fs::metadata`, asserting dir `0700` and script `0600`. Unconditional: no
umask fiddling, because the pre-fix code yields at least `0755`/`0644` under
any realistic umask, so the pin fails on the revert.

*Mutation:* the dir mode-setting block dropped from `Sandbox::create` →

```
assertion `left == right` failed: sandbox dir must not be readable by other accounts, got 755
  left: 493
 right: 448
```

The dir comes back 0755, world-readable in `/tmp`. Reverted; tree verified
clean.

*Mutation:* the script mode-setting block dropped from `write_script` →

```
assertion `left == right` failed: sandbox script must not be readable by other accounts, got 644
  left: 420
 right: 384
```

The script comes back 0644. Reverted; tree verified clean.

### P3 R3-2 — the disposition pin's assert now names the signal it guards

The shipped assertion was `assert_eq!(after.sa_sigaction, before.sa_sigaction,)`
— trailing comma, no message — while the round-2 record quoted the failure as
"signal 15 was left with the session's handler installed". No such string
existed in the tree, so the record's quoted mutation output was not
reproducible. Every other assert in the file names its subject.

**Fix.** The assert now says what failed, using the loop variable that is in
scope:

```rust
assert_eq!(
    after.sa_sigaction, before.sa_sigaction,
    "signal {signal} was left with the session's handler installed",
);
```

**Pin.** The existing `a_termination_signal_disposition_is_restored_after_the_session`
reads the disposition back via `sigaction` (null act, installs nothing) around
an install/restore pair; the mutation empties `restore_signals` and the
readback keeps the handler pointer.

*Mutation:* `restore_signals` body emptied →

```
assertion `left == right` failed: signal 15 was left with the session's handler installed
  left: 94105681362448
 right: 0
```

The message text — "signal 15 was left with the session's handler installed"
— is exactly what the round-2 record quoted, now emitted by the shipped
assert. (The `left:` pointer value varies run to run with ASLR; the message
and the right-hand `0`/`SIG_DFL` are stable.) Reverted; tree verified clean.

## Mutation summary

| Mutation | Pin | Result |
| --- | --- | --- |
| dir mode-setting dropped from `Sandbox::create` | `the_scratch_sandbox_and_script_stay_private` | **caught** — "sandbox dir must not be readable by other accounts, got 755", `left: 493, right: 448` |
| script mode-setting dropped from `write_script` | `the_scratch_sandbox_and_script_stay_private` | **caught** — "sandbox script must not be readable by other accounts, got 644", `left: 420, right: 384` |
| `restore_signals` emptied | `a_termination_signal_disposition_is_restored_after_the_session` | **caught** — "signal 15 was left with the session's handler installed" (the round-2 record's quoted text, now reproducible) |

Every mutation transient; each run against its pinned test, then the mutated
file was restored from a byte copy and `sha256sum`-verified identical to the
pre-mutation state.

## Gates

Run by me at the end, in a clean env (`LAMBO_POSTGRES_DSN`/`MOOSHIK_POSTGRES_DSN`/
`DATABASE_URL` unset — an exported live DSN makes the unrelated
`moving_the_store_is_refused_until_confirmed_and_never_echoes_the_dsn` test
refuse, as recorded in round three's Gates section):

* `cargo test --locked` → **538 lib passed, 0 failed, 2 ignored** (540 total;
  the two ignored are the pre-existing live-Cloud ones) **+ 1 integration
  passed** (`report_pin`). The new pin adds the one test over round two's 537.
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `scratch.rs` 685, `tui/mod.rs` 734 — both under the
  1000-line cap.
* The three pins individually (both new/edited tests) on the final tree: all
  pass.

## What was executed vs. only read

**Executed.** The three mutations above, each against its pinned test, each
reverted and hash-verified. Both pins on the fixed tree. The full suite in a
clean env, clippy, fmt, and the file-size count.

**Read, not executed.** Nothing this round — both findings are pure code in
this checkout, exercised by the pins above.