# Adversarial review — Mooshik M1 + M6, round 2

**Reviewer**: independent read-only round-2 reviewer. The only worktree file
created by this review is this record; application source, dependencies, and
configuration were not edited. No commit or push was performed.
**Baseline**: `0f93bd4da52e3950b3280c36f41e1587a928af2a` (`docs(plan): M0
claimed — TOML strings and line discipline are conventions now`).
**Scope**: the complete dirty M1/M6 implementation: modified `Cargo.toml`,
`Cargo.lock`, `src/cli.rs`, `src/lib.rs`, and `src/text/en.toml`; new
`src/config.rs`, `src/home.rs`, and `src/vault.rs`; and the round-1 record
`m1-m6-round1.md`. Requirements are `docs/SPEC.md` and `dev-diary/PLAN.md`,
especially M1 lines 78–92 and M6 lines 208–224. Review structure follows
`../lambo/dev-diary/adversarial-review/README.md` and its Mooshik records.
**Verdict**: **REQUEST_CHANGES** — one P1 and two P2 residues remain, with
three additional P3 findings. The required zero-P1/P2/P3 clean condition is not
met.

## Method

I read the round-1 record, the full current diff, every current source file,
the M1/M6 requirements, and the Lambo review format. I independently traced
configuration parsing and environment precedence, every home/vault path
operation, lock lifetime, key-provider selection, encryption format, atomic
replacement, permissions, zeroization, and all CLI dispatch/error paths.

All behavioral probes used fresh directories under `/tmp` with explicit
`MOOSHIK_HOME` and passphrase mode. No real home directory or system keyring
was accessed. Probes included:

* fresh `init`, two-process passphrase round-trip, `secret set/get/list`, wrong
  passphrase, absent secret, empty secret value, unsafe names, and help output;
* two concurrent CLI writers against one vault, both of which retained their
  updates;
* mutation of a vault into malformed bytes and mode 0644, verifying rejection
  and mode repair;
* final and intermediate symlink attacks against the home, vault, lock, and
  config paths, including a config parent symlink;
* dependency feature inspection with `cargo tree -e features -i keyring`,
  source inspection of keyring 3.6.3's selected backend, and the injected fake
  keyring backend tests. The real keyring was deliberately not touched.

The malformed-file, mode, symlink, environment, and concurrent-writer cases
are temporary-state mutation probes. I did not weaken or mutate application
source to manufacture a result.

## Round-1 finding closure table

| Finding | Result | Independent evidence |
| --- | --- | --- |
| P1-M6-1 — default provider was the process-local mock | **CLOSED** | `Cargo.toml` selects `linux-native-sync-persistent` + `crypto-rust` on Linux and `apple-native` on macOS; `cargo tree -e features -i keyring` showed the Linux persistent feature path. The keyring crate source selects its persistent backend under that feature. No real keyring was used. |
| P1-M6-2 — documented `vault` path/type was wrong | **OPEN** | Fresh `MOOSHIK_HOME=$H cargo run -- init` created config, database, and logs but **no `$H/vault`**. `HomeLayout::init` returns `Ok` for a missing vault at `src/home.rs:45–50`; `Vault::open` is only reached by `secret` at `src/cli.rs:92–102`. The current home unit test explicitly asserts `!layout.vault.exists()`. PLAN requires the first-run `vault` entry to be encrypted and 0600. Lazy creation on first secret use does not satisfy that first-run layout requirement. |
| P2-M1-1 — concurrent read/modify/write lost updates | **CLOSED** | The lock is acquired before opening/loading and retained in `Vault` through persist. Independent two-process CLI writers both exited 0; `secret list` returned `alpha`, `beta`, and the seed, and both values were readable. The in-tree concurrent test also passed. |
| P2-M1-3 — symlink redirect / TOCTOU defenses | **PARTIAL / OPEN** | Direct final and intermediate symlinks are rejected by independent probes, and final opens use `O_NOFOLLOW` on Unix. However, `validate_path` is a path-by-path metadata check followed by `create_dir_all`, `rename`, or another path lookup. A component can be swapped after validation; `Vault::open` even calls `create_dir_all(parent)` before its later validation. No directory-handle-relative operation or atomic revalidation closes that redirect race. See P2-R2-1. |
| P2-M6-1 — existing vault mode was not repaired | **CLOSED** | A temporary valid vault chmodded 0644 was reopened; it was repaired to 0600 before use. A malformed vault was also rejected while its mode was repaired to 0600. |
| P3-M1-1 — unset HOME fell back to current directory | **CLOSED** | `env -u HOME -u MOOSHIK_HOME cargo run -- config show` exited 1 with the localized home-unavailable message. `resolve_home` now returns `HomeUnavailable` rather than `.`. |
| P3-M6-2 — control characters in names enabled output injection | **CLOSED** | Independent CLI attempts with newline and slash names exited 1 with the localized safe-name error; the source restricts names to 1–64 ASCII alphanumeric/dot/underscore/hyphen bytes. |
| P3-M6-3 — absent names were reported as invalid names | **CLOSED** | `secret get absent` exited 1 with “That secret does not exist”; invalid names still use the distinct invalid-name message. `Vault::get` now returns `NotFound`. |
| P3-M6-4 — value help was dead and empty env input was ambiguous | **CLOSED** | `secret set --help` includes the TOML value-help string. `MOOSHIK_SECRET_VALUE=''` is rejected as missing, and empty stdin/`--value` is also rejected. |
| P3-M6-5 — stale predictable temp and missing directory fsync | **CLOSED** | Temp names include 96 random bits and `create_new`; source syncs the temporary file and then opens/syncs the parent directory after rename. The write path removes a failed temp. |
| P3-M6-6 — plaintext/key buffers were not zeroized | **PARTIAL / OPEN** | `Zeroizing` now wraps the vault key, passphrase provider, serialized plaintext, stored values, and returned token, and token Debug/Display are redacted. But the CLI clones `--value` while the original remains in `ArgMatches`, clones stdin into a new `String` while the original remains ordinary, and passes `env::var(...).unwrap_or_default()` into a provider that copies it; these input copies are dropped without zeroization. See P3-R2-3. |

## New and residual findings

### P2-R2-1 — path validation is still TOCTOU-raceable and can redirect vault writes

`HomeLayout::validate_path` walks components with `symlink_metadata`, then
`ensure_dir` calls `create_dir_all` by path (`src/home.rs:75–103`). `Vault::open`
creates its parent before validating it (`src/vault.rs:205–210`), and
`atomic_private_write` validates the parent and later performs `rename` by path
(`src/vault.rs:358–397`). An attacker able to replace an intermediate component
between those operations can make directory creation, the temporary-file
rename, or the subsequent directory sync address a different tree. Final
`O_NOFOLLOW` checks do not protect intermediate components or the path-based
rename. This is the remaining portion of P2-M1-3, and it violates the requested
path-canonicalization/no-follow invariant for encrypted material. Use stable
directory handles or an equivalent no-follow, race-safe protocol and test the
replacement window.

### P2-R2-2 — `config show` bypasses home path validation and follows parent symlinks

`dispatch` calls `show_config` directly (`src/cli.rs:70–80`), while only the
secret path calls `HomeLayout::init`. `Config::load` rejects a symlink at the
final `config.toml` component and opens it with Unix `O_NOFOLLOW`, but it never
validates parent components. With a temporary `$H/home-link -> $OUTSIDE`
symlink and `$OUTSIDE/config.toml` containing passphrase config,
`MOOSHIK_HOME=$H/home-link cargo run -- config show` exited 0 and printed the
outside file. It also repairs that outside file's permissions through the
parent symlink. The same home passed to `init` is rejected, proving the config
command is bypassing the established guard. Validate the complete home layout
before config access, including read-only `config show`.

### P3-R2-1 — nested command groups silently succeed without a subcommand

`mooshik config` and `mooshik secret` both exited 0 with empty stdout and
stderr. The builder enables neither nested `subcommand_required(true)` nor a
dispatch error for the `_` cases. This is an ambiguous CLI contract and makes
mistyped automation look successful. Show nested help or return a nonzero,
localized error.

### P3-R2-2 — vault and stdin input have no size limit

`Vault::open` uses unbounded `read_to_end` (`src/vault.rs:211–216`) and
`atomic_private_write` builds an unbounded ciphertext buffer. CLI stdin uses
unbounded `read_to_string` (`src/cli.rs:115–121`), and values have no maximum.
A same-user corrupted or unexpectedly large vault can force unbounded memory
allocation before authentication, and a pipe can make the CLI consume
unbounded input. Add explicit maximum encoded file, JSON, and input sizes with
localized errors; enforce them before allocation.

### P3-R2-3 — zeroization does not cover CLI input copies

The newtype wrappers improve the stored-vault path, but they do not erase all
copies. `args.get_one::<String>("value")` is cloned into `Zeroizing` while
Clap retains the original `String`; stdin is read into an ordinary `String`
then copied by `trim_end_matches(...).to_owned()`; and the passphrase from
`env::var` is copied into `PassphraseProvider` while the source `String` is
dropped normally. This is residual sensitive-buffer retention under the exact
passphrase/zeroization review scope. Read into zeroizing buffers and avoid
unnecessary copies (or explicitly zeroize the source buffers).

## Fresh regression/security assessment

The keyring feature selection now genuinely points at native persistent Linux
and macOS stores; the fake backend round trip passed, and the system backend was
not invoked. Lock lifetime now serializes cross-process read/modify/write.
Final vault/config/lock symlinks, existing modes, malformed format, name
controls, absent names, random temp names, and parent directory fsync all have
effective defenses. XChaCha20-Poly1305, fresh 24-byte nonces, Argon2's default
Argon2id parameters, authenticated decrypt, and redacted token Debug/Display
remain sound under source inspection and the temporary round trips.

The remaining issues are not cosmetic: first-run layout still omits the required
vault file; path operations are not race-safe; config inspection can escape a
symlinked parent; sensitive CLI input copies remain ordinary allocations; and
unbounded file/input sizes permit local denial of service. The three fresh P3s above
are all recorded intentionally; none is waived.

## Gate table

All gates below were independently run on the reviewed dirty tree. No real
keyring or real home was touched.

| Gate / probe | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test` | PASS — 18 passed, 0 failed, 0 ignored |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo run --quiet -- --help` | PASS — init/config/secret surface shown |
| Fresh passphrase `secret set/get/list` across CLI processes | PASS |
| Wrong passphrase and malformed vault | PASS — nonzero, localized generic errors; no secret output |
| Existing vault mode mutation 0644 → reopen | PASS — repaired to 0600 |
| Concurrent CLI writers | PASS — both updates retained |
| Newline/slash names and absent name | PASS — safe name and distinct not-found errors |
| Empty `MOOSHIK_SECRET_VALUE` and `secret set --help` | PASS — rejection and value help present |
| Final/intermediate symlinks for `init`, vault, lock, config | PASS for direct attacks; **FAIL** for `config show` with symlinked parent (P2-R2-2) |
| Fresh `init` first-run layout | **FAIL** — no `vault` file is created (P1-M6-2) |
| `cargo tree -e features -i keyring` | PASS — native persistent Linux feature selected |
| `wc -l` file-size discipline | PASS — largest source file 629 lines, none over 1000; vault data/input limits still absent (P3-R2-2) |

## Conclusion

**REQUEST_CHANGES.** P1-M6-2 remains open because first-run `init` does not
create the required encrypted 0600 `vault` file. P2-M1-3 remains open in its
TOCTOU portion, and P2-R2-2 demonstrates a separate parent-symlink escape in
`config show`. P3-R2-1 through P3-R2-3 are also recorded and must be cleared;
the review cycle cannot return CLEAN/APPROVE until the complete P1/P2/P3 set is
zero.
