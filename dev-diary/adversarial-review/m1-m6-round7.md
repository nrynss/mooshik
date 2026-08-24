# Adversarial review — Mooshik M1 + M6, round 7

**Reviewer**: independent, review-only adversarial pass. The only worktree file
written by this pass is this record; implementation, dependencies, commits, and
pushes were not changed.
**Date**: 2026-08-25
**Scope**: the complete current M1/M6 worktree, all prior round-1 through
round-6 records, and the M1/M6 requirements in `docs/SPEC.md` and
`dev-diary/PLAN.md`. The review shape follows the Lambo adversarial-review
convention.
**Verdict**: **REQUEST_CHANGES** — one P2 and one P3 remain. This is not a
clean integration pass.

## Method and safety boundary

I read all six prior review records, the Lambo review README, the full current
implementation and diff, and the M1/M6 requirements. I traced configuration and
home lifecycle, marker/recovery policy, root and parent descriptor lifetime,
every descriptor-relative filesystem operation, vault locking/replacement,
permissions, key-provider selection, authenticated envelope parsing, input
bounds/zeroization, CLI errors/help, and the R6 fresh-home creation protocol.

Behavioral probes used fresh temporary homes only; no real home, OS keyring, or
external credential store was touched. I ran fresh passphrase `init`, layout and
mode inspection, two-process secret round trips, wrong-passphrase behavior, and
the existing path-swap/symlink/recovery coverage. I also inspected the selected
`libc` 0.2.189 definitions locally: Linux `RENAME_NOREPLACE` is `1`, and macOS
`renameatx_np` takes the five arguments used here with `RENAME_EXCL = 0x4`.

## R6 closure table

| R6 item | Result | Evidence |
| --- | --- | --- |
| Fresh-home replacement between `mkdirat` and `openat` | **Partially closed; residual below** | A private random staging directory plus an inode snapshot and `fstat` check rejects replacement before the descriptor is opened. The no-replace install rejects a competing final target. The source pathname is still mutable after that check and before the install. |
| Staging randomness and modes | Pass, with test-coverage note | `temporary_directory_name` uses 16 bytes from `OsRng`; `mkdirat(..., mode)` is followed by descriptor `fchmod`, with M1 callers passing 0700. The source is sound; no retained test asserts the exact staging mode/randomness contract. |
| Parent safety precondition | Pass for mode/owner/sticky policy | `creation_parent_is_private` rejects non-sticky group/other-writable parents and non-sticky parents owned by another uid; sticky parents such as `/tmp` are allowed because their entry ownership rules protect cross-uid rename. Same-uid races remain the relevant threat in the residual. |
| Linux no-replace install | Pass by source inspection | `SYS_renameat2` with `RENAME_NOREPLACE` (1), no `renameat` fallback; `ENOSYS`/unsupported-filesystem errors fail closed. |
| macOS no-replace install | Pass by source/libc inspection | `renameatx_np(fromfd, from, tofd, to, RENAME_EXCL)` is the correct FFI and constant; unsupported filesystems fail closed. A macOS build was not available in this Linux environment. |
| Competing final target | Pass for the tested pre-install window | The retained target test injects a competing ordinary directory and receives `AlreadyExists`; no-replace prevents overwrite. The missing post-open source-swap case is the P2 below. |
| Error cleanup | **Fail** | Cleanup calls `unlinkat(..., 0)` for directories, which returns `EISDIR`; staging directories remain on target competition and other failure paths (P3 below). |
| Full M1/M6 sweep | Pass except findings below | Config/env validation, marker/recovery/refusal policy, permissions, lock lifetime, AEAD/AAD/tamper rejection, keyring feature selection, passphrase fallback, bounds, redaction, CLI help, and prior findings remain closed under current source/tests. |

## Findings

### P2-R7-1 — The staging pathname can be replaced after identity validation and before no-replace install

`create_or_open_directory_at_with_hook` opens the random staging directory and
checks its `(st_dev, st_ino)` at `src/secure_path.rs:281–297`. It then calls
`chmod_fd` and invokes `install_directory_noreplace` using the **staging
pathname** at `:298–303`. The retained `File` is not an argument to
`renameat2`/`renameatx_np`, so the kernel operation is not bound to the inode
whose descriptor was checked.

A same-uid actor can, after the `fstat` check and before the install, rename the
original staging directory to an attacker-controlled location and create an
ordinary replacement directory at the random staging name. The no-replace call
then successfully installs the replacement at the requested leaf while the
returned descriptor still refers to the original directory. `open_dir_with_status`
returns that descriptor as the newly-created home (`:360–367`), and
`HomeLayout::init` writes the marker, config, support files, and later encrypted
vault through it (`src/home.rs:42–69`, `:189–198`). Thus initialization can write
material into the moved original outside the apparent home path, or report a
successful home whose pathname names a different directory. This is the same
creation-time path-identity invariant R5/R6 were intended to establish, and the
no-replace flag only protects the destination name; it does not protect the
source name after the descriptor check.

The current tests do not distinguish this case. The retained-root test swaps the
path **after** `open_dir_with_status` returns, and the new creation tests inject
replacement before staging `openat` or between the snapshot and staging `openat`.
There is no injected pause after `descriptor_matches_identity`/`chmod_fd` and
before `renameat2`/`renameatx_np`.

Bind the installed entry to the exact descriptor with a platform-appropriate
primitive, or at minimum re-open/compare the installed destination and fail
closed before returning any descriptor when the source was replaced; ensure the
caller cannot proceed with an unbound moved directory. Add a deterministic
post-open/pre-install replacement test asserting no marker/config/vault is
written into the attacker-controlled moved location. Do not add a plain
`renameat` fallback on Linux or macOS.

### P3-R7-1 — Directory staging cleanup is ineffective on every directory failure path

The staging error paths at `src/secure_path.rs:272–285` and `:300–302` call
`unlink_at_fd`. That helper always invokes `unlinkat(parent, leaf, 0)`
(`:655–665`). The staging object is a directory, and `unlinkat` with flags `0`
does not remove directories; it returns `EISDIR` (directory removal requires
`AT_REMOVEDIR`). The ignored cleanup errors therefore leave random
`.mooshik-stage-*` directories behind when identity/open/chmod/install fails.

The existing competing-target test removes its entire temporary parent after
the assertion, so it masks this defect and does not verify cleanup. Leaked
staging directories can accumulate after unsupported-filesystem errors,
permission errors, or a competing target and can make recovery/debugging noisy.
Use a dedicated descriptor-checked directory removal path (and leave a changed
pathname untouched), or an equivalent safe cleanup protocol; add tests that
assert no stage remains after target competition and the relevant injected
failure paths.

## Requirement verification

| Requirement | Result |
| --- | --- |
| M1 config parse/validation and non-empty env overlay | Pass — current source/tests |
| M1 first-run layout, recovery/refusal policy, and modes | Pass for exercised paths — fresh regular vault 0600, root/logs 0700, support/lock 0600; creation race is P2-R7-1 |
| M6 native keyring default | Pass by target feature tree/source inspection — persistent Linux backend and macOS native feature; real stores deliberately untouched |
| M6 Argon2id fallback | Pass — isolated two-process round trip and wrong-passphrase rejection |
| M6 authenticated envelope/AAD and tamper rejection | Pass — header, salt, nonce, and ciphertext are authenticated |
| Locking/concurrent writers/atomic vault replacement | Pass — lock spans load/modify/persist; randomized `create_new` temp, file sync, rename, and parent sync present |
| No-follow and descriptor-relative paths | Pass for existing roots, symlinks, retained-root swaps, and tested creation windows; residual source-swap race above |
| Input bounds, names, redaction, and zeroization | Pass — prior R1–R6 findings remain closed |

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --all-targets` | PASS — 34 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS |
| `cargo run --quiet -- secret set --help` | PASS — environment/stdin contract visible |
| `git diff --check` | PASS |
| Fresh passphrase init and mode probe | PASS — regular vault 0600; root/logs 0700; support/lock 0600 |
| Two-process passphrase set/get and wrong passphrase | PASS |
| Existing empty/marked-partial/unmarked legacy homes | PASS — recovery, preservation, and explicit migration refusal |
| Symlink/intermediate/root-descriptor swap probes | PASS — current tests and source trace |
| `cargo tree -e features -i keyring` | PASS — persistent Linux backend selected |
| Real Linux/macOS OS keyring | NOT RUN by design — no real credential store touched |
| Linux/macOS cross-build | NOT RUN — current host is Linux; libc source confirms the macOS FFI/constant |
| Creation replacement after staging descriptor check | **NOT CLOSED** — no deterministic test; source race remains P2-R7-1 |
| Failed staging directory cleanup | **FAIL** — `unlinkat(..., 0)` cannot remove a directory (P3-R7-1) |

## Conclusion

**REQUEST_CHANGES.** The R6 no-replace operations are correctly selected and
fail closed, the inode snapshot/open test catches the earlier tautological
identity check, and all normal M1/M6 gates are green. The creation protocol is
not yet race-safe through installation: the source pathname can be swapped after
the descriptor check, and the corresponding test window is absent. Separately,
directory cleanup is ineffective because file unlink flags are used for staging
directories. Remediate the P2 and P3, repeat the adversarial pass, and integrate
only after a zero-finding verdict.

— independent reviewer, 2026-08-25
