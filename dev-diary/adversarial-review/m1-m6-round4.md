# Adversarial review — Mooshik M1 + M6, round 4

**Reviewer**: independent, review-only adversarial pass. The only worktree file
written by this pass is this record; application source, dependencies, and
configuration were not edited, committed, or pushed.
**Date**: 2026-08-25
**Baseline**: `0f93bd4da52e3950b3280c36f41e1587a928af2a` (`docs(plan): M0
claimed — TOML strings and line discipline are conventions now`), with the
complete uncommitted M1/M6 implementation currently in the worktree.
**Scope**: all current dirty files (`Cargo.toml`, `Cargo.lock`, `src/cli.rs`,
`src/lib.rs`, `src/text/en.toml`, `src/config.rs`, `src/home.rs`,
`src/secure_path.rs`, and `src/vault.rs`), plus the round-1, round-2, and
round-3 records. Requirements are `docs/SPEC.md` and `dev-diary/PLAN.md`,
especially the M1 layout/config contract and M6 encrypted local-vault contract.
Review structure follows `../lambo/dev-diary/adversarial-review/README.md`.
**Verdict**: **REQUEST_CHANGES** — one P2 and three P3 findings remain. The
required zero-P1/P2/P3 clean condition is not met.

## Method and safety boundary

I read all three prior review records, the Lambo review convention, the full
current implementation/diff, and the M1/M6 requirements. I traced home marker
creation and validation, first-run and partial-home recovery, uid/mode checks,
descriptor lifetime through CLI dispatch, all fd/error paths, no-follow
operations and revalidation, platform-specific keyring selection, vault
envelope parsing/AAD/version behavior, provider lifecycle, metadata tamper
failure, input bounds, secret output/error paths, command help, and config
loading.

Behavioral probes used only fresh temporary homes under `/tmp`; no real home,
OS keyring, or external credential store was touched. Probes included:

* fresh absent-home passphrase `init`, two-process `set/get/list`, wrong
  passphrase, config show, layout/mode inspection, and concurrent writers;
* an existing empty 0700 home, a marked partial home, and an unmarked legacy
  home;
* tampering with the authenticated salt, nonce, and ciphertext;
* the nested-command surface, `secret set --help`, missing-home config show,
  symlink/layout rejection, and no-secret-on-error checks;
* source/unit verification of the retained-root swap, uid/mode boundary,
  descriptor-relative path operations, lock lifetime, and keyring feature
  selection.

## Round-3 finding closure

| Finding | Round-4 result | Independent evidence |
| --- | --- | --- |
| P2-R2-1 — root-directory replacement between init and reopen | **CLOSED** | `HomeLayout::init` returns the open root descriptor and CLI `initialize`/`dispatch_secret` carry it into `Config::load_at` and `Vault::open_at`. The retained-root swap unit test passes and writes only to the moved original directory. |
| P2-R3-1 — caller-controlled home could chmod/populate arbitrary existing directory | **CLOSED for security; new UX residue below** | Existing roots must already be mode 0700, owned by the effective uid, and contain the exact private marker; `/` and an unrelated 0755 directory are rejected without mutation. The new strict marker requirement rejects legitimate pre-created/legacy roots; see P2-R4-1. |
| P3-R3-1 — `--value` plaintext remained in ordinary Clap storage | **CLOSED** | There is no `--value` argument. `secret set` accepts only the name and reads a bounded environment/stdin value into `Zeroizing` storage. |
| P3-R3-2 — keyring-mode salt was unauthenticated metadata | **CLOSED** | The version-2 magic, salt, and nonce form the AEAD AAD. Independent fake-keyring salt mutation, nonce mutation, and ciphertext mutation all fail authentication without output. |
| P3-R3-3 — home test asserted the old missing-vault lifecycle | **OPEN** | `src/home.rs` still asserts `!layout.vault.exists()` immediately after `HomeLayout::init`; it never verifies the provider-backed first-run `init` path creates an authenticated 0600 vault. See P3-R4-1. |

## Requirement verification

| Requirement | Verification | Result |
| --- | --- | --- |
| M1 config TOML loads, denies unknown fields, and validates provider values | `Config::from_toml_and_env`, `Config::load_at`, unit tests, bounded config read, and config-show probe | Pass; missing-home command wording residue is P3-R4-3 |
| M1 non-empty environment overlay wins; empty preserves file | Unit tests and provider/home overlay probes | Pass |
| M1 first-run layout and private modes | Fresh absent-home CLI init produced marker, config, database, lock, regular vault 0600, root/logs 0700; existing mode repair and root boundary tests pass | Pass for absent default path; **partial** for pre-created/legacy homes (P2-R4-1) |
| M6 OS keyring default | Linux Cargo feature tree selects `linux-native-sync-persistent` + `crypto-rust`; macOS manifest selects `apple-native`; fake backend round trip passes | Pass by feature/source inspection; real keyring deliberately unverified |
| M6 passphrase fallback | Argon2 provider round trip and wrong-passphrase authentication failure; env provider selection | Pass |
| M6 AEAD envelope, AAD, nonce, and version semantics | Version-2 header is AAD; salt/nonce/ciphertext mutation probes fail; malformed minimum/header checks pass | Pass |
| Keyring/passphrase lifecycle and failure handling | Missing passphrase fails without secret output; keyring failures are localized; provider key remains outside vault | Pass for implemented lifecycle; no delete/rotate command is in this scope |
| Existing vault is repaired to 0600 | Existing 0644 vault probe and unit test repair before decrypt | Pass |
| Stable fd lifetime/revalidation/no-follow | Descriptor-relative `openat`/`renameat`/`unlinkat`, `O_NOFOLLOW`, retained-root swap test, symlink probes | Pass on supported Linux path; Linux/macOS are the specified platforms |
| Input bounds/stdin | 4 MiB vault bound, 1 MiB secret/stdin bound, bounded reads before allocation, empty input rejection | Pass for the exercised CLI paths |
| No secret leakage | Wrong-passphrase, malformed/tampered vault, list, and error probes; `SecretToken` Debug/Display redaction; no plaintext in ciphertext | Pass for current M1/M6 surface |
| CLI surface | `--help`, nested command rejection, set/get/list round trip | Functional; `secret set --help` omits the value-input contract (P3-R4-2) |

## Findings

### P2-R4-1 — Strict home marker policy rejects legitimate first-run and migration roots

`HomeLayout::init` opens an existing directory with `create = false` and then
requires all of the following before it will repair or populate it:

* exact mode 0700 and ownership by the effective uid;
* `.mooshik-home` containing exactly `mooshik home\n`;
* existing `config.toml` and `mooshik.db` files.

An existing private directory without the new marker is therefore classified as
an unsafe path rather than adopted or migrated. This is easy to reproduce with
an otherwise valid first-run override:

```text
H=$(mktemp -d)                 # existing directory, mode 0700
MOOSHIK_HOME=$H mooshik init
=> exit 1: The Mooshik home contains a symbolic link or unsafe path component.
```

The same failure occurs for an unmarked legacy home containing the prior
config/database layout. A marked root with missing support files is also
rejected before the repair code runs, so a crash after marker creation but
before the remaining files are created is not recoverable through `init`.
The error claims a symbolic link or unsafe component even when the path is a
normal private directory, and there is no migration/backup instruction. A
user must manually move or delete the directory to proceed, risking loss of
existing data. This violates the practical M1 first-run/migration contract and
is a P2 because the normal existing-private-home path is unusable while the
security restriction is broader than necessary.

Accept an existing empty/private root by creating and validating the marker;
for a marker-owned root, safely create missing support files and recover an
interrupted initialization. For a populated unmarked root, retain the secure
refusal but provide a dedicated migration error and an explicit safe
backup/rename path. Add CLI tests for empty pre-created, legacy, and partial
marker-owned homes, including the no-data-loss behavior.

### P3-R4-1 — The home lifecycle test still does not test the first-run vault contract

`home::tests::init_creates_private_usable_layout_and_repairs_modes` asserts
that `layout.vault` does not exist immediately after `HomeLayout::init`, then
creates the vault manually through `Vault::open_at`. The real CLI `mooshik init`
does the provider-backed open and creates the regular 0600 vault, but no test
drives that `initialize` path. Consequently, a regression that stopped CLI
initialization from creating or authenticating the required vault could leave
the current home unit test green.

Replace the stale assertion with a first-run lifecycle assertion that checks the
regular 0600 vault and authenticated reopen through the same provider-backed
path used by `initialize`; keep a separate layout-only test only if that API
distinction is intentional and documented.

### P3-R4-2 — `secret set --help` does not disclose how the secret value is supplied

The actual command surface is:

```text
Usage: mooshik secret set <name>
```

Its only description is “Encrypt and store a secret value”; it does not mention
that the value is read from `MOOSHIK_SECRET_VALUE` when that variable is set,
otherwise read from stdin, that trailing newlines are removed, or that empty
input is rejected. The implementation can therefore block an operator who did
not know to pipe stdin, and the documented non-interactive contract is not
discoverable from the CLI. The round-3 closure claim that the value contract is
visible is not borne out by the current help output.

Add a TOML-backed `after_help`/argument help paragraph that states the exact
environment/stdin contract and size/empty-input behavior, and pin it with a
CLI help test. Keep values out of argv.

### P3-R4-3 — `config show` reports a missing home as an initialization failure

`show_config` calls `open_existing_root`, but a missing root is mapped through
`HomeError::Io`, whose display key is `home.init_failed`. On a fresh or removed
home, `mooshik config show` consequently prints:

```text
Could not initialize the Mooshik home directory. Check its path and permissions.
```

The command did not attempt initialization, so this gives the wrong diagnosis
and does not tell the user to run `mooshik init` (or use the correct home
override). Give read-only config inspection its own missing-home error and
actionable next step; add a missing-home `config show` CLI regression test.

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --all-targets` | PASS — 25 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS — init/config/secret surface renders |
| `git diff --check` | PASS |
| Fresh absent-home passphrase init/layout | PASS — marker, config, database, lock, vault 0600, root/logs 0700 |
| Two-process passphrase set/get/list and wrong passphrase | PASS — wrong passphrase exits 1 with no stdout |
| Concurrent writers | PASS — both updates retained in independent process probe and unit test |
| Existing empty 0700 home | **FAIL** — rejected as unsafe before initialization (P2-R4-1) |
| Unmarked legacy/partial home | **FAIL** — no adoption or recovery path (P2-R4-1) |
| Root boundary/unrelated-directory mutation | PASS — `/` and unrelated 0755 directory rejected without mutation |
| Symlink final/intermediate/home-parent paths | PASS — no-follow descriptor-relative operations reject probes |
| Root descriptor path swap | PASS — retained descriptor unit test remains green |
| Existing vault mode repair | PASS — 0644 repaired to 0600 |
| Header metadata/ciphertext tamper | PASS — salt, nonce, and ciphertext mutations fail authentication |
| `cargo tree -e features -i keyring` | PASS — persistent Linux backend selected; macOS native feature in manifest |
| Real Linux/macOS OS keyring | NOT RUN by design — no real credential store touched |
| `secret set --help` value contract | **FAIL** — stdin/env behavior is not discoverable (P3-R4-2) |
| Missing-home `config show` wording | **FAIL** — reports init failure instead of read-only missing-home guidance (P3-R4-3) |

## Conclusion

**REQUEST_CHANGES.** The R3 root-fd, root-boundary, zeroization, authenticated
header, locking, mode-repair, input-bound, and no-follow fixes are verified and
the full Rust gates are green. The implementation is not clean: the newly
introduced marker policy rejects ordinary pre-created and unmarked legacy homes
without a migration/recovery path (P2), the lifecycle test still misses the
actual first-run vault contract (P3), `secret set` hides its input contract
(P3), and `config show` reports the wrong missing-home failure (P3). Remediate
all four findings and repeat the adversarial cycle before commit/push.

