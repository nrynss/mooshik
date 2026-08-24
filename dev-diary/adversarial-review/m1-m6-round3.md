# Adversarial review — Mooshik M1 + M6, round 3

**Reviewer**: independent, review-only adversarial pass. The only file written by
this pass is this record; application source, dependencies, and configuration were
not edited, committed, or pushed.
**Date**: 2026-08-25
**Baseline**: dirty worktree based on `0f93bd4da52e3950b3280c36f41e1587a928af2a`.
**Scope**: the complete current M1/M6 worktree, including `src/secure_path.rs`,
the current CLI, all tests, Cargo target-specific keyring features, and the R1/R2
records. Requirements are `docs/SPEC.md` and `dev-diary/PLAN.md`, especially the
M1 layout/mode contract and the M6 local encrypted-vault contract. Review shape
follows `../lambo/dev-diary/adversarial-review/README.md`.
**Verdict**: **REQUEST_CHANGES** — two P2 and three P3 findings remain.
The required zero-P1/P2/P3 condition is not met.

## Method and safety boundary

I read R1 and R2, the complete current source/diff, the M1/M6 requirements, and
the Lambo review convention. I traced every public path operation and CLI dispatch
path, including config show, initialization, vault open, lock, replacement, and
the provider selection/error paths. Behavioral probes used only fresh directories
under `/tmp` with explicit passphrase mode. No real home directory, OS keyring,
or external credential store was touched. I did not run the intentionally dangerous
`MOOSHIK_HOME=/` mutation probe; the root chmod behavior is directly established by
source trace.

Executed gates and probes included:

* `cargo fmt --all -- --check`, `cargo test --all-targets`,
  `cargo clippy --all-targets -- -D warnings`, `cargo run -- --help`, and
  `cargo tree -e features -i keyring`;
* fresh passphrase `init`, layout/mode inspection, two-process set/get/list,
  wrong-passphrase authentication, empty input, oversized input, malformed-name,
  and nested-command behavior;
* existing-vault mode repair, final/intermediate/home-parent symlink rejection,
  config-show symlink rejection, and concurrent writer regression coverage;
* source-level review of keyring backend selection, unsupported-provider behavior,
  fd-relative operations, atomic replacement/directory sync, allocation bounds,
  and all plaintext copies.

## R1/R2 closure table

| Finding | Round-3 result | Evidence |
| --- | --- | --- |
| P1-M6-1 — default keyring was process-local mock | **CLOSED** | Linux Cargo features select `linux-native-sync-persistent` + `crypto-rust`; the feature tree shows the persistent path. macOS selects `apple-native`. No real keyring was touched. |
| P1-M6-2 — vault layout/type was wrong | **CLOSED** | A fresh isolated `mooshik init` with passphrase overlay creates a regular `$H/vault` file mode 0600, plus config/database/lock and mode-700 logs. The old `HomeLayout::init` unit expectation that vault is absent is stale; see P3-R3-3. |
| P2-M1-1 — concurrent writers lost updates | **CLOSED** | The lock is acquired before load and retained through persist; the in-tree concurrent test and the round-trip probe retain both updates. |
| P2-M1-3 / P2-R2-1 — path validation/TOCTOU | **PARTIAL / OPEN** | Stable directory descriptors and `*at`/`O_NOFOLLOW` operations close intermediate symlink redirects. However, CLI `initialize` and `dispatch_secret` call `layout.init()` and then reopen the root by path (`src/cli.rs:140–144`, `:101–106`), leaving a root-directory replacement window; see P2-R2-1 below. |
| P2-M6-1 — existing vault mode not repaired | **CLOSED** | A temporary valid 0644 vault is reopened and repaired to 0600 before use; malformed-vault and symlink probes also fail closed. |
| P3-M1-1 — missing HOME fell back to CWD | **CLOSED** | `resolve_home` returns `HomeUnavailable`; no-current-directory fallback remains. |
| P3-M6-2 — control characters in names | **CLOSED** | Names are restricted to 1–64 ASCII alphanumeric, dot, underscore, and hyphen bytes; newline/slash probes fail. |
| P3-M6-3 — absent name reported as invalid | **CLOSED** | `VaultError::NotFound` and its distinct localized message are present and exercised. |
| P3-M6-4 — value help/empty env ambiguity | **CLOSED** | `secret set --help` exposes the value contract; empty env, stdin, and `--value` are rejected. |
| P3-M6-5 — temp/durability defects | **CLOSED** | Temp names have 96 random bits and `create_new`; temp and parent directory are synced; failed temps are unlinked. |
| P3-M6-6 / P3-R2-3 — sensitive CLI copies not zeroized | **OPEN** | Stored values, passphrase, stdin buffer, serialized plaintext, key, and token are wrapped, but `parse_secret_arg` returns an ordinary `String` retained in Clap's `ArgMatches`; `dispatch_secret` then passes `&String` to `Vault::set` and does not erase that original. See P3-R3-1. |
| P2-R2-2 — config show parent-symlink escape | **CLOSED** | `show_config` opens the home root through `open_dir`; a symlinked home/parent now exits with the localized unsafe-path error and does not read the outside config. |
| P3-R2-1 — empty nested command groups | **CLOSED** | Both `config` and `secret` use `subcommand_required(true)`; empty invocations exit 2 with clap help. |
| P3-R2-2 — unbounded vault/stdin input | **CLOSED** | Config is bounded to 64 KiB, vault bytes to 4 MiB, secret/stdin values to 1 MiB, and checks precede unbounded allocation. |

## Findings

### P2-R2-1 — root-directory replacement remains possible between fd-relative phases

`secure_path::open_dir` itself is correctly descriptor-relative and uses
`O_NOFOLLOW`, but the CLI does not retain the descriptor returned by its first
initialization phase. `initialize` calls `layout.init()` (`src/cli.rs:140`),
which opens and validates one root, drops that handle, and then calls
`layout.open_existing_root()` (`:141`) before loading config and creating the
vault. `dispatch_secret` has the same `layout.init()` then reopen sequence
(`:102–103`). A same-user actor can rename the validated root directory and
place a different ordinary directory at the original path during that gap.
The subsequent `open_dir` accepts the replacement because it is not a symlink,
and config/vault writes proceed under the wrong tree. The descriptor-relative
vault code therefore does not close the complete path identity race claimed by
R2. Carry the opened root descriptor from init through config/provider/vault
operations, or bind/revalidate the root inode before each phase with an
equivalent race-safe protocol; add a regression test for an ordinary-directory
swap, not only symlinks.

### P2-R3-1 — caller-controlled home can chmod and populate an arbitrary existing directory

`HomeLayout::init` opens `self.root` and unconditionally calls
`secure_path::set_dir_mode(&root, 0o700)` (`src/home.rs:33–35`). There is no
boundary check that the resolved `MOOSHIK_HOME` is a dedicated new directory,
not an existing directory such as the current working tree, `/tmp`, or `/`.
If the process has permission, `MOOSHIK_HOME=/` changes the root directory to
0700 and then creates/opens `/logs`, `/config.toml`, `/mooshik.db`, and `/vault`;
the same behavior on an existing user-selected directory changes its mode and
adds application files to it. The root probe was deliberately not executed to
avoid damaging the host, but the path is direct and unconditional. Reject the
filesystem root and other unsuitable existing roots, or require a dedicated
home leaf and only chmod directories created by Mooshik. Test both existing
directory and root-boundary cases safely via a fixture.

### P3-R3-1 — the `--value` plaintext survives in an ordinary Clap allocation

The R2 zeroization gap remains. `parse_secret_arg` (`src/cli.rs:73–79`) allocates
an ordinary `String`; Clap owns it in `ArgMatches` until dispatch returns.
`dispatch_secret` (`:112–119`) borrows that value and `Vault::set` allocates a
second `Zeroizing<String>`, but the original argument allocation is dropped
without zeroization. This is a direct CLI path for credential material and is
not fixed by zeroizing the vault's stored value. Use a zeroizing argument type
or explicitly erase the Clap-owned buffer after use (and keep the parser and
help behavior tested).

### P3-R3-2 — keyring-mode salt is unauthenticated metadata

`persist` encrypts only the JSON plaintext (`src/vault.rs:321–327`); MAGIC, salt,
and nonce are serialized outside the AEAD ciphertext (`:331–336`) and are not
AAD. In keyring mode `KeyringProvider::load_or_create` ignores the salt
(`:79–98`), so an attacker who changes the 16-byte salt in a vault file can
make a silent metadata mutation while decryption still succeeds. Passphrase
mode happens to bind the salt indirectly through key derivation, but the default
keyring mode does not. Authenticate the complete header as AAD (or include it in
the encrypted payload) and add a keyring-provider tampered-salt regression test.

### P3-R3-3 — the home unit test asserts the pre-remediation layout

`home::tests::init_creates_private_usable_layout_and_repairs_modes`
(`src/home.rs:107–128`) explicitly asserts `!layout.vault.exists()` at `:114`,
even though the CLI's first-run `init` now creates the required encrypted 0600
vault. This test would continue to pass if the M1 first-run vault lifecycle
regressed inside `HomeLayout::init`, and it does not exercise the provider-backed
`initialize` path that closes R1's P1. Replace the stale assertion with a
first-run lifecycle test that checks the regular 0600 vault file and authenticated
reopen, while retaining a separate layout-only test only if that distinction is
intentional and documented.

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --all-targets` | PASS — 21 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS |
| `secret set --help` | PASS — value contract is visible |
| Fresh passphrase `init` layout | PASS — vault regular file 0600; root 0700; logs 0700; config/db/lock 0600 |
| Two-process passphrase set/get/list | PASS |
| Wrong passphrase | PASS — exit 1, no stdout, localized generic error |
| Concurrent writers | PASS — both updates retained; unit regression passes |
| Empty input, oversized input, unsafe names | PASS — nonzero and localized; no hang or unbounded read |
| Nested `config`/`secret` without subcommand | PASS — exit 2 |
| Existing vault mode repair | PASS — 0644 becomes 0600 |
| Final/intermediate/home-parent symlink attacks | PASS — rejected; config-show parent symlink no longer escapes |
| `cargo tree -e features -i keyring` | PASS — persistent Linux backend path selected |
| Real OS keyring persistence | NOT RUN by design — no real credential store touched |
| Ordinary-directory root swap between `init` and reopen | NOT safely injectable through current CLI; source trace confirms the window (P2-R2-1) |
| Root-boundary mutation probe | NOT RUN by design — would risk host-wide permission/data changes; source trace confirms it (P2-R3-1) |

## Conclusion

**REQUEST_CHANGES.** The M1 vault lifecycle, keyring feature selection, locking,
fd-relative leaf operations, config-show symlink defense, size bounds, and basic
CLI behavior are materially improved and pass the gates above. The review is not
clean: the CLI drops and reopens the home root across phases, `init` can chmod and
populate an arbitrary existing directory, one direct CLI secret copy remains
ordinary memory, keyring-mode header metadata is not authenticated, and the
home test still asserts the old missing-vault behavior. Remediate all P2/P3
items, rerun this review cycle, then integrate only after a zero-finding pass.
