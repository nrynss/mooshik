# Adversarial review — Mooshik M1 + M6, round 1

**Reviewer**: independent read-only adversarial reviewer. The only file written by this
review is this record; application source, `Cargo.toml`, and `Cargo.lock` were not edited.
**Baseline**: `0f93bd4` (`docs(plan): M0 claimed — TOML strings and line discipline are conventions now`).
**Scope**: the complete uncommitted M1/M6 worktree diff: modified `Cargo.toml`,
`Cargo.lock`, `src/cli.rs`, `src/lib.rs`, `src/text/en.toml`, and new `src/config.rs`,
`src/home.rs`, `src/vault.rs`. Requirements were taken from `docs/SPEC.md` and
`dev-diary/PLAN.md`, especially PLAN M1 (lines 78–92) and M6 (lines 208–224).
**Review standard**: `../lambo/dev-diary/adversarial-review/README.md` and the existing
Mooshik/Lambo review records.
**Verdict**: **REQUEST_CHANGES** — 2 P1, 3 P2, and 6 P3 findings remain open.

## Method

I read the applicable repo docs and every new/modified source file in full, then traced
configuration/env handling, CLI dispatch, home creation, key-provider selection, AEAD
format, name validation, output redaction, and file replacement. I attacked the implementation
with isolated temporary homes only; no real home directory or system keyring was used.

Executed probes included:

* fresh passphrase-mode `init`, `secret set/get/list`, wrong-passphrase authentication, and
  mode/layout inspection;
* two concurrent `secret set` processes against one temporary vault;
* a newline-containing secret name, inspected as raw `secret list` bytes;
* default-provider set in one CLI process followed by get in a second CLI process;
* source and dependency-feature inspection of `keyring` 3.6.3.

## Requirement verification

| Requirement | Verification | Result |
| --- | --- | --- |
| M1 config TOML loads and validates | `Config::load`/`from_toml_and_env`; unknown fields and provider values are rejected; empty env preserves file | Partial: missing-home fallback and symlink/permission issues remain (P3-M1-1, P2-M1-3) |
| M1 non-empty environment wins | Unit tests and source trace for provider/home overrides | Pass for the covered provider/home cases |
| M1 first-run layout and modes | Isolated `init` probe | **Fail**: `vault` is a directory 0700, not the required encrypted `vault` file 0600 (P1-M6-2) |
| M6 keyring default | `Cargo.toml:13`, `cargo tree -e features -i keyring`, two-process repro | **Fail**: no platform feature is enabled; keyring uses the in-memory mock (P1-M6-1) |
| M6 passphrase fallback | Argon2 default (Argon2id v19), isolated round-trip and wrong-key rejection | Pass for basic round-trip/authentication |
| M6 AEAD/nonce/format | XChaCha20-Poly1305, fresh 24-byte nonce per persist, authenticated decrypt, malformed-file rejection | Pass for the tested primitive; durability/concurrency gaps remain (P2-M1-1, P3-M6-5) |
| M6 CLI set/get/list | Isolated passphrase-mode CLI probe; values are absent from ciphertext and wrong passphrase exits 1 | Pass for the basic path; UX/name issues remain (P3-M6-2, P3-M6-3, P3-M6-4) |
| M6 0600 encrypted vault | Fresh file is 0600, but existing file mode is never checked/reset | Partial/fail for pre-existing vaults (P2-M6-1) |
| M6 egress redaction | `redact_output` unit test exists; no M4 tool/result path exists yet, so integration is not exercisable in this scope | Helper pass; end-to-end integration deferred to M4 (not counted as a finding here) |

## Findings

### P1-M6-1 — The agreed default OS keyring is actually the process-local mock

**Evidence**: `Cargo.toml:13` declares only `keyring = "3"`; no `apple-native`,
`linux-native-sync-persistent`, or other platform backend feature is selected.
`cargo tree -e features -i keyring` reports only the crate's `default` feature. The
installed keyring 3.6.3 source documents that it has no default platform store features and
falls back to `mock` when none apply. `src/vault.rs:35-50` then calls `keyring::Entry` without
detecting that fallback.

**Reproduction (isolated temporary home, no system keyring)**:

```text
H=$(mktemp -d)
MOOSHIK_HOME=$H cargo run -- init
printf 'keyring-secret\n' | MOOSHIK_HOME=$H cargo run -- secret set token
MOOSHIK_HOME=$H cargo run -- secret get token
=> exit 1: The vault could not be authenticated. Check the passphrase or keyring entry.
```

The first command stores a newly generated key in the mock backend for that process. The
second process sees no key, creates another key, and cannot decrypt the vault. This violates
the explicit product decision to use the OS keyring by default and makes the default CLI
unusable across invocations on both Linux and macOS. Select the actual persistent native
backends per supported target (or fail closed with a clear unsupported-backend error); add a
second-process persistence test that does not rely on the real user's keyring.

### P1-M6-2 — The required `~/.mooshik/vault` 0600 file is implemented as a directory

**Evidence**: PLAN M1 says the layout contains `vault` and explicitly requires `vault` mode
0600 (`dev-diary/PLAN.md:80-90`); M6 defines `~/.mooshik/vault` as the encrypted 0600 vault
(`dev-diary/PLAN.md:208-212`). `src/home.rs:34` creates `self.vault` as a 0700 directory,
while `src/cli.rs:93` silently changes the storage location to
`vault/secrets.bin`. An isolated `init`/set probe produced:

```text
700 directory .../.mooshik/vault
600 regular file .../.mooshik/vault/secrets.bin
```

This is not just naming: callers and later milestones that use the documented layout receive
a directory where the encrypted vault file must be, and the path named `vault` has the wrong
mode. Make `HomeLayout::vault` the 0600 file, create it atomically on first use, and update
all CLI/tests/docs consistently.

### P2-M1-1 — Concurrent writers lose secrets despite atomic replacement

**Evidence**: `src/vault.rs:186-216` loads a complete snapshot, and `:220-224` mutates
that snapshot before `:244-312` atomically renames it. There is no lock or compare-and-swap.
Atomic rename prevents a torn file but does not serialize read-modify-write operations.

**Reproduction**: after seeding one key in a temporary passphrase vault, run two concurrent
commands, `secret set a` and `secret set b`. Both exit successfully; `secret list` returned
only `a` and `seed` (the `b` update was silently lost). Add an advisory lock covering open,
modify, and persist (with Linux/macOS behavior tested), or another conflict-safe update
protocol, and add a concurrent-writer regression test.

### P2-M1-3 — Home and vault paths follow symlinks and are vulnerable to redirect/TOCTOU

**Evidence**: `src/home.rs:32-45` uses `exists`, `create_dir_all`, and `set_permissions`.
These follow pre-existing symlinks: a `vault` symlink is accepted, its target is chmodded,
and `src/cli.rs:93` writes `secrets.bin` into that target outside the apparent Mooshik home.
The `exists`-then-create checks also leave a replacement window. `Vault::open` (`src/vault.rs:191-216`)
similarly trusts a path without rejecting symlinks.

An isolated pre-created `vault -> outside` symlink is enough to make initialization alter the
outside directory and put the vault there. Reject symlink components (or use no-follow,
directory-handle-relative operations) and revalidate the final path immediately before
creating/renaming. Add Linux/macOS symlink tests, including a replacement race where feasible.

### P2-M6-1 — Existing vault files are not brought back to the required 0600 mode

**Evidence**: `HomeLayout::init` only resets modes for `config.toml` and `mooshik.db`
(`src/home.rs:36-45`). `Vault::open` reads an existing file (`src/vault.rs:192-196`) but
never checks or resets its permissions. A pre-existing `secrets.bin` chmodded 0644 therefore
remains group/world-readable after `init` and normal vault use. Fresh creation happens to set
0600, which is why the current test does not catch this. Enforce 0600 before accepting an
existing vault (or fail closed), without following symlinks, and test the pre-existing-file
case.

### P3-M1-1 — Unset `HOME` silently places the home directory in the current working directory

**Evidence**: `src/config.rs:101-113` uses `env::var_os("HOME")` and falls back to `PathBuf::from(".")`,
so the effective path becomes `./.mooshik`. This violates the documented `~/.mooshik` default
and can put encrypted material in an unexpected repository/current directory when a launcher
does not provide HOME. Use a platform home-directory API or return a clear error when no home
can be resolved; add an unset-HOME test.

### P3-M6-2 — Secret names allow terminal/output injection

**Evidence**: `src/vault.rs:315-321` rejects only empty, length, `/`, and `\\`. Newline,
carriage return, tabs, ANSI escapes, and other control characters are accepted. A probe stored
`bad\nname`; `secret list` emitted raw bytes `62 61 64 0a 6e 61 6d 65 0a`, making one name
look like two records (and ANSI sequences could alter a terminal). Restrict names to a safe
documented character set or reject all control characters, and add list-output tests.

### P3-M6-3 — Missing-secret errors are reported as invalid-name errors

**Evidence**: `Vault::get` maps both invalid input and absent map entries to
`VaultError::InvalidName` (`src/vault.rs:226-234`), whose text says names must be non-empty and
must not contain separators. `secret get absent` consequently gives a false diagnosis and no
useful next step. Add a distinct `NotFound` error and user-facing message, with tests for both
invalid and absent names.

### P3-M6-4 — The documented value-help string is dead and empty env input silently changes mode

**Evidence**: `src/text/en.toml` defines `vault.value_help`, but `secret_command` in
`src/cli.rs:53-59` registers only a name argument. `src/cli.rs:100-108` treats an empty
`MOOSHIK_SECRET_VALUE` as if it were unset and then blocks reading stdin. This makes the
documented non-interactive input behavior invisible in `--help` and can hang automation that
intentionally supplies an empty value. Either expose/document the exact stdin/env contract and
reject empty values explicitly, or use a non-blocking, unambiguous input mode; add CLI tests.

### P3-M6-5 — Atomic write is not fully durable and uses a predictable stale temp name

**Evidence**: `src/vault.rs:293-312` syncs the temporary file but never syncs the parent
directory after `rename`, so a crash can lose the rename on filesystems requiring directory
fsync for durability. The temp path is only `.vault-{pid}.tmp`; a stale file left by a crash
causes every later write in that process-id reuse scenario to fail until manual cleanup. Use a
unique random temp name, sync the parent directory after rename on Unix, and test failure and
restart paths.

### P3-M6-6 — Secret/key/plaintext buffers are retained without zeroization

**Evidence**: `PassphraseProvider` retains a `Vec<u8>` (`src/vault.rs:96-108`), `Vault` retains
the raw `[u8; 32]` key (`:179-184`), and `SecretToken`/`Secrets` retain plaintext `String`s
(`:151-161`). None implements zeroizing drop. This is not a disk/log leak, but it leaves
credential material in heap/stack memory after use and is avoidable for a security-sensitive
vault. Add `zeroize` wrappers/`Zeroizing` where compatible with the API and test that debug/
display behavior remains redacted.

## Gate table

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test` | PASS — 12 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS — help lists init/config/secret commands |
| Isolated passphrase CLI round-trip, wrong-passphrase exit | PASS — wrong passphrase exits 1 with generic non-secret error |
| Isolated concurrent writer probe | FAIL behaviorally — both commands succeed but one secret is lost (P2-M1-1) |
| Isolated default-keyring two-process probe | FAIL behaviorally — second process cannot decrypt (P1-M6-1) |
| Real Linux/macOS system keyring | UNVERIFIED — deliberately not touched; dependency feature inspection proves the current build selects mock |

## Conclusion

The passphrase AEAD round-trip and basic CLI/error gates are green, but this round is not
clean. The default agreed keyring mode is non-persistent mock storage, the documented vault
path/mode contract is broken, concurrent updates silently lose credentials, path symlinks are
trusted, and existing permissions are not repaired. The six P3s are also actionable and must
be cleared before an APPROVE/CLEAN verdict.

— independent reviewer, 2026-08-25
