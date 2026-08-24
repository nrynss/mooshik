# Adversarial review — Mooshik M1 + M6, round 5

**Reviewer**: independent, review-only adversarial pass. The only worktree file
written by this pass is this record; application source, dependencies, and
configuration were not edited, committed, or pushed.
**Date**: 2026-08-25
**Baseline**: the complete uncommitted M1/M6 implementation based on
`0f93bd4da52e3950b3280c36f41e1587a928af2a`.
**Scope**: all dirty application files (`Cargo.toml`, `Cargo.lock`,
`src/cli.rs`, `src/config.rs`, `src/home.rs`, `src/lib.rs`,
`src/secure_path.rs`, `src/text/en.toml`, and `src/vault.rs`), plus the R1–R4
review records. Requirements are `docs/SPEC.md` and `dev-diary/PLAN.md`,
especially M1's home/config contract and M6's encrypted local-vault contract.
Review shape follows the Lambo adversarial-review convention.
**Verdict**: **REQUEST_CHANGES** — one P2 and two P3 findings remain. The
required zero-P1/P2/P3 clean condition is not met.

## Method and safety boundary

I read the full current implementation and diff, all prior review records, the
M1/M6 requirements, and the filesystem/keyring configuration. I traced first
run, empty-root adoption, marker-owned partial recovery, legacy refusal,
unrelated-directory refusal, root descriptor lifetime, no-follow operations,
vault locking/replacement, permissions, envelope authentication, provider
selection, input bounds, plaintext handling, and CLI error/help paths.

Behavioral probes used fresh temporary homes under `/tmp`; no real home,
system keyring, or external credential store was touched. Probes covered:

* absent-home `init`, 0600 vault/0600 support-file/0700 directory modes,
  two-process passphrase round-trip, wrong-passphrase no-stdout behavior, and
  existing-vault mode repair;
* existing empty 0700 home adoption, marker-owned partial-home recovery with
  unrelated data preserved, unmarked non-empty legacy refusal, unrelated 0755
  directory refusal without mutation, and symlinked-home refusal;
* missing-home `config show`, `secret set --help`, and source/unit checks for
  retained root descriptors, lock lifetime, authenticated header metadata,
  no-follow operations, and persistent native keyring feature selection.

## Round-4 finding closure

| Finding | Round-5 result | Independent evidence |
| --- | --- | --- |
| P2-R4-1 — strict marker policy rejected empty/partial homes | **CLOSED for the documented recovery policy** | Empty private roots are marked and initialized; marker-owned roots missing config/database/logs are repaired while unrelated data remains; unmarked non-empty roots return `MigrationRequired` without adding a marker; unrelated 0755 roots are rejected without chmod/population. |
| P3-R4-1 — lifecycle test asserted the old missing-vault behavior | **CLOSED** | The home tests now assert a regular 0600 vault, authenticated provider-backed reopen, and a persistent fake-keyring first-run lifecycle. |
| P3-R4-2 — `secret set --help` hid value input behavior | **CLOSED** | Help now states the environment/stdin source, bounds, empty-input rule, newline behavior, and prohibition on argv values. |
| P3-R4-3 — missing `config show` reported initialization failure | **CLOSED** | Fresh missing-home probe exits nonzero with the localized “does not exist” message and explicit `mooshik init` guidance, with no stdout. |

## Requirement verification

| Requirement | Result |
| --- | --- |
| M1 config parse, unknown-field rejection, provider validation, non-empty env overlay | Pass — unit tests and source trace |
| M1 first-run layout and modes | Pass for absent homes and the tested recovery paths — marker, config, database, lock, vault 0600, root/logs 0700 |
| M1 migration/refusal policy | Pass for empty private, marked partial, unmarked legacy, unrelated existing, and symlink cases; creation-time race remains P2-R5-1 |
| M6 native OS keyring default | Pass by target feature tree/source inspection (`linux-native-sync-persistent` + `crypto-rust`; macOS `apple-native`); real keyring deliberately not touched |
| M6 Argon2id passphrase fallback | Pass — round trip and wrong-key authentication rejection |
| M6 AEAD envelope and metadata authentication | Pass — version-2 header is AAD; salt/nonce/ciphertext mutation tests reject tampering |
| M6 concurrent updates and atomic durability path | Pass — lock is held over load/modify/persist; randomized temp, file sync, rename, parent sync, and cleanup are present |
| M6 vault permissions/path safety | Pass for exercised existing modes, symlinks, intermediate components, retained-root swap, and layout conflicts |
| M6 input bounds/name safety/redaction surface | Pass for exercised bounds, ASCII names, absent-name errors, redacted token formatting, and current CLI surface; P3-R5-1/P3-R5-2 remain |

## Findings

### P2-R5-1 — Creating a missing home still has a directory replacement window

`secure_path::open_dir_with_status` handles a missing component with
`mkdirat(current, name, mode)` and then a separate
`open_directory_at(current, name)` (`src/secure_path.rs:129–136`). If a
same-user actor replaces the freshly-created directory between those calls,
the second open accepts an ordinary replacement directory. `HomeLayout::init`
trusts the returned `created` flag and skips `validate_existing_root`
(`src/home.rs:42–55`), so it then writes the marker, config, database, and
eventual encrypted vault into the replacement tree. `O_NOFOLLOW` prevents a
symlink at the second lookup but does not prevent this ordinary-directory
identity swap.

This is a remaining creation-time form of the path redirect/TOCTOU issue that
the retained-root descriptor fix correctly closes after initialization. Because
the redirected tree can receive encrypted vault material, it remains P2 under
the project's explicit race-resistant path requirement. Use a creation
protocol that retains/binds the exact newly-created directory identity before
continuing (with platform-appropriate primitives), or otherwise fail closed on
an identity change; add a regression test for replacement during missing-home
creation where the test harness can inject the window.

### P3-R5-1 — Environment-provided values do not follow the documented CR/LF rule

The `secret set` help says “Trailing CR/LF is removed”
(`src/text/en.toml:35`), but the environment branch returns the value directly
(`src/cli.rs:142–150`). A temporary passphrase vault storing
`MOOSHIK_SECRET_VALUE=$'line\n'` produced two newline bytes from `secret get`
(one stored newline plus `println!`'s output newline), proving that the
environment and stdin contracts differ despite the shared help text. Either
strip trailing CR/LF in the environment branch as documented, or qualify the
help text to say the rule applies only to stdin, and pin the chosen behavior in
a test.

### P3-R5-2 — Invalid UTF-8 stdin takes credential bytes out of zeroizing storage

`read_secret_value` keeps stdin bytes in `Zeroizing<Vec<u8>>` until
`String::from_utf8(std::mem::take(&mut *bytes))`
(`src/cli.rs:152–168`). On invalid UTF-8, `String::from_utf8` returns an error
containing an ordinary `Vec<u8>`; the `map_err` immediately discards that
error, so the rejected input bytes are dropped without zeroization. This is an
edge path, but it is a direct secret-input path and leaves the prior
zeroization guarantee incomplete. Preserve the failed bytes in a zeroizing
container (or explicitly zeroize before discarding) and add an invalid-UTF-8
input regression test.

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --all-targets` | PASS — 29 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS |
| `cargo run --quiet -- secret set --help` | PASS — value contract visible |
| `git diff --check` | PASS |
| Fresh absent-home passphrase init/layout | PASS — vault 0600; root/logs 0700 |
| Two-process set/get and wrong-passphrase behavior | PASS — wrong passphrase exits nonzero with no stdout |
| Existing empty home | PASS — adopted and marked |
| Marked partial home | PASS — support files restored; unrelated data preserved |
| Unmarked non-empty legacy home | PASS — explicit migration refusal; no marker/data mutation |
| Unrelated existing 0755 directory | PASS — rejected without chmod/population |
| Home/vault symlinks and retained-root swap | PASS — existing coverage and probes remain green |
| Existing vault mode repair | PASS — 0644 restored to 0600 |
| Real Linux/macOS OS keyring | NOT RUN by design — no real credential store touched |
| Missing-home `config show` | PASS — actionable read-only missing-home guidance |

## Conclusion

**REQUEST_CHANGES.** R4's recovery policy, lifecycle test, help text, and
missing-home wording are materially addressed, and all Rust/CLI gates are
green. The review is not clean: missing-home creation still has an ordinary
directory replacement window before the retained root descriptor exists (P2),
the environment value path contradicts its documented trailing-newline rule
(P3), and invalid UTF-8 stdin escapes zeroizing storage on rejection (P3).
Remediate these findings and repeat the adversarial pass before integration.

— independent reviewer, 2026-08-25
