# Adversarial review — Mooshik M1 + M6, round 8

**Reviewer**: independent, review-only adversarial pass. The only worktree file
written by this pass is this record; application source, dependencies, commits,
and pushes were not changed.
**Date**: 2026-08-25
**Scope**: the complete current uncommitted M1/M6 implementation, all prior
round-1 through round-7 records, and the M1/M6 requirements in
`docs/SPEC.md` and `dev-diary/PLAN.md`. The review shape follows the Lambo
adversarial-review convention.
**Verdict**: **REQUEST_CHANGES** — one P3 finding remains. No P1 or P2 was
found in this pass; the required zero-P1/P2/P3 clean condition is not met.

## Method and safety boundary

I read every prior Mooshik review record, the Lambo review README, and the full
current `config`, `home`, `secure_path`, `vault`, CLI, manifest, and test code.
I specifically re-traced R7's staging protocol from `mkdirat` through the
identity snapshot, descriptor open/check, `fchmod`, no-replace install,
post-install reopen/check, returned-descriptor semantics, and every error
cleanup path. I also checked the Linux and macOS no-replace errno contracts and
the current keyring feature tree.

Behavioral checks used only fresh temporary homes and the in-tree injected
filesystem-window tests. No real home, OS keyring, or external credential
store was touched. I ran fresh passphrase CLI/layout checks, the full Rust
gates, help, diff, and feature-tree checks.

## Round-7 finding closure

| R7 item | Result | Independent evidence |
| --- | --- | --- |
| P2-R7-1 — staging source pathname replacement after identity check and before install | **CLOSED** | `create_or_open_directory_at_with_hooks` checks the identity again after the no-replace install and before returning. `staging_replacement_after_open_before_install_fails_closed` swaps the checked staging directory away and installs an ordinary replacement at the staging name; the operation returns `PermissionDenied`, leaves both the requested target and moved staging directory empty, and does not return a descriptor to the caller. Linux uses `renameat2(RENAME_NOREPLACE)` with no fallback; macOS uses `renameatx_np(RENAME_EXCL)`. |
| P3-R7-1 — directory cleanup used file unlink flags | **PARTIALLY CLOSED / RESIDUAL BELOW** | Cleanup now uses `unlinkat(..., AT_REMOVEDIR)`, and `staging_directory_is_removed_after_injected_failure` confirms normal cleanup. The check-then-remove sequence is still raceable; see P3-R8-1. |

## Caller and concurrency protocol verification

The post-install check is meaningful under the stated private-parent model:
if the source pathname was replaced before the no-replace rename, reopening the
destination yields a different inode and the function fails closed. If the
destination is swapped after the check, the returned root descriptor still
refers to the checked directory, and all current callers carry that descriptor
through `HomeLayout::init`, `Config::load_at`, and `Vault::open_at`; they do not
reopen `self.root` by path. The retained-root swap test verifies this caller
contract. A path consumer that ignored the returned descriptor would not be
safe, but no such consumer exists in the current M1/M6 code.

Concurrent creation and target competition fail closed: no-replace returns an
error, the original staging name is cleaned only when its identity still
matches, and there is no plain `renameat` fallback. Vault read/modify/write
locking remains held through persistence, and the concurrent-writer test keeps
both updates.

## Finding

### P3-R8-1 — staging cleanup still has a check/use race and can remove an unrelated directory

`remove_staging_directory_if_unchanged` opens the staging pathname, compares
the opened descriptor to the saved `(st_dev, st_ino)`, and then removes the
pathname with `unlinkat(parent, leaf, AT_REMOVEDIR)` (`src/secure_path.rs:731–743`).
The descriptor does not bind the later pathname operation. A same-UID actor,
which remains in scope because R7 explicitly treats same-UID races as the
relevant threat under the private-parent assumption, can rename the checked
staging directory away and install a different empty directory at `leaf`
between the comparison and `unlinkat`. The final call then removes the
replacement directory. `AT_REMOVEDIR` prevents deleting a non-empty directory,
but it does not make the identity check and removal atomic.

This contradicts the helper's promise that a changed pathname is deliberately
left untouched. It is limited to failure cleanup of a random staging name and
does not expose vault plaintext, so this is P3 rather than P2, but it remains an
integrity/safety defect. The safe choices are to use a genuinely descriptor-
bound removal primitive where available or to fail closed and leave the staging
directory for later operator cleanup; do not remove a pathname after a
non-atomic identity check. Add an injected hook/barrier between
`descriptor_matches_identity` and `unlinkat`, swap in an empty replacement, and
assert that the replacement remains.

The current cleanup test only injects failure before the staging directory is
opened (`secure_path.rs:866–883`), so it cannot race the check/use window.

## Requirement verification

| Requirement | Result |
| --- | --- |
| M1 config parsing, validation, and non-empty env overlay | Pass — current source/tests and prior closure remain valid. |
| M1 first-run layout, recovery/refusal policy, and modes | Pass for exercised paths — regular 0600 vault, private root/logs/support files, marker/recovery policy; no new finding. |
| M6 native OS keyring default | Pass by manifest and `cargo tree` inspection — persistent Linux backend selected; macOS native feature present. Real stores deliberately untouched. |
| M6 Argon2id fallback | Pass — isolated two-process passphrase behavior remains valid. |
| Authenticated vault format and nonce handling | Pass — v2 header is AAD and tamper checks remain green. |
| Concurrent writers and durable replacement | Pass — lock spans load/modify/persist; randomized create-new temp, file sync, rename, and parent sync remain present. |
| No-follow and descriptor-relative paths | Pass for existing roots and tested creation windows; cleanup check/use residual is P3-R8-1. |
| Bounds, safe names, redaction, and zeroization | Pass under the current tests and prior closure records. |

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --all-targets` | PASS — 36 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS |
| `cargo run --quiet -- secret set --help` | PASS — environment/stdin contract visible |
| `git diff --check` | PASS |
| `cargo tree -e features -i keyring` | PASS — persistent Linux backend selected |
| Fresh passphrase init/layout and two-process round trip | PASS |
| Existing empty/marked-partial/unmarked legacy homes | PASS — current tests and refusal/migration policy |
| R7 post-identity/pre-install staging swap | PASS — deterministic injected test fails closed |
| Staging cleanup check/use replacement race | **NOT CLOSED** — no deterministic test or descriptor-bound removal |
| Real Linux/macOS OS keyring | NOT RUN by design — no external credential store touched |

## Conclusion

**REQUEST_CHANGES.** R7's post-install identity verification and no-replace
installation close the reviewed creation source-swap window, and the returned
descriptor semantics are sound for all current callers. The cleanup flag fix is
functionally correct for the ordinary path, but the final pathname removal is
still not identity-bound and can delete a same-UID replacement empty directory.
Remediate P3-R8-1 and repeat the review before integration.

— independent reviewer, 2026-08-25
