# Adversarial review — Mooshik M1 + M6, round 6

**Reviewer**: independent, review-only adversarial pass. The only worktree file
written by this pass is this record; application source, dependencies, and
configuration were not edited, committed, or pushed.
**Date**: 2026-08-25
**Baseline**: the complete uncommitted M1/M6 implementation based on
`0f93bd4da52e3950b3280c36f41e1587a928af2a`.
**Scope**: all current dirty application files, the complete R1–R5 review
history, and the M1/M6 requirements in `docs/SPEC.md` and `dev-diary/PLAN.md`.
Review shape follows `../lambo/dev-diary/adversarial-review/README.md`.
**Verdict**: **REQUEST_CHANGES** — one P2 finding remains. No P1 or P3
findings were found in this pass. The required zero-P1/P2/P3 clean condition is
not met.

## Method and safety boundary

I read R1, R2, R3, R4, and R5 in full, then re-read the current implementation,
tests, requirements, and the Lambo review convention. I traced home creation and
recovery, marker validation, root descriptor lifetime, all descriptor-relative
filesystem operations, lock and atomic replacement behavior, permissions,
keyring/passphrase selection, authenticated envelope parsing, input bounds,
zeroization, CLI output, and error paths.

Behavioral probes used fresh temporary homes under `/tmp`; no real home,
system keyring, or external credential store was touched. I ran:

* fresh passphrase `init`, two-process `secret set/get`, CR/LF environment input,
  invalid-UTF-8 stdin, missing-home `config show`, and mode/layout checks;
* source and unit-test inspection of the fresh-home mkdir/open replacement
  protocol, including the identity-check test's exact timing;
* the complete Rust gates, help output, feature-tree inspection, and diff check.

The missing-home creation race was reviewed as a source-level adversarial case:
the narrow `mkdirat`/`openat` window is not externally controllable without
instrumentation, and no application source was modified to manufacture one.

## Round-5 finding closure

| Finding | Round-6 result | Independent evidence |
| --- | --- | --- |
| P2-R5-1 — missing-home mkdir/open replacement window | **OPEN** | The new `create_or_open_directory_at` check is not identity-preserving across the window; see P2-R6-1. |
| P3-R5-1 — environment values ignored documented trailing CR/LF rule | **CLOSED** | `normalize_environment_value` strips all trailing `\r`/`\n` before accepting the value (`src/cli.rs:153–164`); unit test passes, and a fresh CLI round trip stored the normalized value. |
| P3-R5-2 — invalid UTF-8 stdin escaped zeroizing storage | **CLOSED** | `normalize_stdin_bytes` validates the borrowed bytes and explicitly zeroizes the rejected allocation before returning (`src/cli.rs:167–187`); invalid-byte CLI probe exits nonzero with no stdout, and the unit test passes. |

All earlier P1/P2/P3 findings recorded by R1–R4 remain closed under the
current source and tests: native persistent keyring features are selected,
the first-run vault is a regular 0600 file, concurrent writers are locked,
existing modes are repaired, root and parent descriptors are retained, home
boundary/migration policy is explicit, the v2 header is authenticated, input
sizes are bounded, names are safe, nested command groups reject empty use, and
secret/token/input copies currently remain in `Zeroizing` storage on the
reviewed paths.

## Finding

### P2-R6-1 — fresh-home creation still accepts an ordinary directory replacement

`secure_path::create_or_open_directory_at` reports a newly created directory
after this sequence (`src/secure_path.rs:120–129`):

1. `mkdirat(parent, name, mode)` creates the intended directory;
2. `open_directory_at(parent, name)` opens whatever directory is currently at
   that pathname;
3. `same_directory_entry(parent, name, &directory)` compares that same current
   pathname to the just-opened descriptor.

If a same-user actor renames the directory created in step 1 and installs an
ordinary replacement directory between steps 1 and 2, step 2 opens the
replacement and step 3 compares the replacement pathname with the replacement
descriptor. The comparison succeeds, `created = true` is returned, and
`HomeLayout::init` proceeds to write the marker, config, database, logs, and
eventual encrypted vault into the replacement tree (`src/secure_path.rs:194–197`,
`src/home.rs:42–55`, `:159–167`, `:184–193`). `O_NOFOLLOW` only rejects a
symlink; it does not bind the opened descriptor to the directory produced by
the preceding `mkdirat`.

The current regression test does not exercise that window. It calls
`create_or_open_directory_at` to completion, then renames and replaces the path
after the descriptor has already been returned (`src/secure_path.rs:532–548`).
That proves a retained descriptor survives a later swap, but it cannot prove
the creation-time handoff is safe. The source-level counterexample above is
deterministic in the presence of an injected scheduling point and leaves
encrypted material redirectable during first-run creation, so this remains P2.

Use a creation protocol that binds the exact newly-created directory before
continuing (for example, create a private unpredictable temporary directory,
retain its descriptor, and atomically install it with a no-replace operation),
or fail closed when the final directory identity cannot be proven. Add a test
with an injectable pause/barrier between creation and open that swaps in an
ordinary directory, and assert that initialization fails without writing
marker/config/vault material into the replacement.

## Requirement verification

| Requirement | Result | Evidence |
| --- | --- | --- |
| M1 config parse/validation and non-empty env overlay | Pass | Current source/tests; R1–R5 closure remains valid. |
| M1 first-run layout and recovery modes | Pass for exercised absent, empty, and marked-partial homes | Fresh CLI init created marker, config, database, lock, regular vault 0600, root/logs 0700; recovery tests pass. Creation-time replacement remains P2-R6-1. |
| M6 native OS keyring default | Pass by target feature/source inspection | `cargo tree -e features -i keyring` selects Linux persistent native backend; macOS manifest selects `apple-native`; real stores deliberately untouched. |
| M6 Argon2id passphrase fallback | Pass | Fresh two-process passphrase round trip and wrong-key authentication behavior remain green. |
| M6 authenticated AEAD envelope and nonce handling | Pass | v2 magic/salt/nonce header is AAD; existing tamper tests pass. |
| Concurrent updates and durable atomic replacement | Pass | Lock is retained through load/modify/persist; random `create_new` temp, file sync, rename, and parent sync remain present; concurrent test passes. |
| Permissions and no-follow paths | Pass except fresh-home creation race | Existing file/dir mode repair, retained-root swap, symlink rejection, and descriptor-relative operations pass. |
| Bounds, name safety, redaction, and zeroization | Pass | 4 MiB vault/1 MiB input bounds, restricted names, redacted token formatting, CR/LF normalization, and invalid UTF-8 rejection are covered. |

## Gate table

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --all-targets` | PASS — 33 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS |
| `git diff --check` | PASS |
| `cargo tree -e features -i keyring` | PASS — persistent Linux backend selected; macOS native feature present in manifest |
| Fresh passphrase `init` and layout/modes | PASS — vault 0600; root/logs 0700; support files/lock 0600 |
| Two-process passphrase set/get | PASS — normalized CR/LF value round-trips |
| Invalid-UTF-8 stdin | PASS — nonzero, no stdout, localized generic error |
| Missing-home `config show` | PASS — nonzero, no stdout, actionable `mooshik init` guidance |
| Existing empty/marked-partial/migration homes | PASS — current tests pass and preserve unrelated data/refuse unmarked non-empty roots |
| Creation-time ordinary-directory replacement | **NOT CLOSED** — current identity check is after `openat` and compares against the current pathname; test swaps only after function return (P2-R6-1) |
| Real Linux/macOS OS keyring | NOT RUN by design — no real credential store touched |

## Conclusion

**REQUEST_CHANGES.** R5's CR/LF and invalid-UTF-8 findings are independently
closed, and the complete Rust/CLI gates plus fresh functional probes are green.
One P2 remains: the missing-home `mkdirat`/`openat` handoff can accept an
ordinary replacement directory because the post-open identity check is
tautological under replacement. The existing test does not cover that timing
window. Remediate P2-R6-1 and repeat the adversarial pass before commit/push.

— independent reviewer, 2026-08-25
