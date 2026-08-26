# M7 adversarial review — round 2

Branch `m7-cli-sweep`, HEAD `958564f`. Independent re-verification of the
round-1 remediation: fresh source trace of every closure, reviewer-run binary
probes against a fresh `/tmp` home, and four new mutations (all reverted;
`git status` clean before, between, and after every mutation; nothing
committed).

## Verification performed

* **P1-a** — traced `normalize_environment_value` / `normalize_stdin_bytes`
  (`src/cli.rs:476-526`) through `Failure::from → is_user_error`. Every
  rejection path now constructs a typed variant via
  `anyhow::Error::new(VaultError::…)` (`MissingValue`, `InputTooLarge`, `Io`);
  no bare `anyhow!` remains on the secret-input surface. Live probes
  (reviewer-run, real binary): env-empty **exit 2**, message
  `vault.missing_value` verbatim; stdin-empty **exit 2**, same message;
  oversized stdin (MAX+1 bytes) **exit 2**, `vault.input_too_large` verbatim.
  The oversized-env branch is unreachable through `execve` in practice
  (1 MiB ≫ `MAX_ARG_STRLEN`) but is typed and unit-covered — defensible.
  Non-UTF-8 stdin exits 1 (`VaultError::Io`, internal) — classification is
  the single decision point as designed; its shared message ("Check its
  permissions") gives slightly off advice for a decode failure. P3-grade,
  pre-existing shape, not residue.
* **P2-b** — `tests/report_pin.rs` drives the **real binary**
  (`CARGO_BIN_EXE_mooshik`) as a subprocess against a fixture home whose
  config carries planted DSN material on a refused port, removes ambient DSN
  env authorities, and asserts stderr **byte-for-byte equal** to
  `memory.backend_failed + "\n"`, exit 1, empty stdout, no DSN substring.
  This is tight: any extra byte fails. Audit of every print site reachable on
  that path (`main.rs` → `lib::run` → `cli::run`): exactly one stderr site
  exists — `Failure::report`; the stdout sites are success-path renders that
  never execute when `recall` fails. No second formatter can carry the fixture
  material.

## Mutation table

| # | Mutation | Pin expected to catch | Result |
| --- | --- | --- | --- |
| M1 | Naive `{:#}` appended to `rendered()`'s String inside `report` | `report_prints_only_the_top_level_message…` | **CAUGHT** (byte-equal assertion fails) — but vacuous by construction, so redone faithfully |
| M2 | Faithful round-1 mutation: `eprintln!("{error:#}")` on the error itself inside `report` | same pin | **CAUGHT** — observed stderr grew the wrapped chain (`…credentials.: backend: pool timed out…`), byte-equal + substring assertions fail |
| M3 | Raw `LamboError` Display restored in `tools::lambo_err` | `tool_boundary_stderr_notices_route_through_en_toml…` + `lambo_err_returns_the_fixed_notice_not_the_lambo_display` | **CAUGHT** (both failed) |
| M4 | Raw panic-payload print restored in `ToolExecutor::execute` | `tool_boundary_stderr_notices_route_through_en_toml…` | **CAUGHT** |
| M5 | Empty env value untyped back to bare `anyhow!` | `empty_secret_values_classify_user_with_the_missing_value_message` | **CAUGHT** |

All reverted; final tree contains none of them.

## Per-finding re-verification

### P1-a — fixed
See above. The classifier is again the single decision point; both canonical
operator mistakes exit 2 live.

### P2-c — fixed, with honest assessment of the structural-only pin
`From<LamboError>` routes `Conflict(detail)` → `MemoryError::SessionConflict`
and everything else → `Backend(_)` (pinned by
`lambo_conflicts_map_to_the_session_conflict_variant_not_the_generic_backend`,
including that Backend stays exit 1). Verified against lambo @ `f90a6622`:
the conflict message is built at `memory.rs:889-896` from **session id +
holder token + lease age only**; the holder token is
`agent@host#pid` where pid comes from the OS and host from
`HOSTNAME`/`HOST`/`hostname` (best-effort); the J2 endpoint column is
deliberately excluded from the token and absent from the message. No DSN or
credential material can enter it. `en.toml` renders
`memory.session_conflict` with `.replace("{detail}", …)` (no format
injection); classifier maps the variant to exit 2.

Structural-vs-behavioral honesty check: I tried to find a behavioral route
and could not build a cheap one. `run_chat_async` catches every per-turn
companion error inside its loop (`chat.rs:91-94` prints and continues), so
the only reachable `Err` returns are the tokio runtime build (not injectable)
and a stdin read *error* (not EOF — EOF is `Ok(())`). A recording-executor
test cannot make `block_on` fail without fd-level tricks against the real
process stdin. The remediation note's "not reachable in fixtures" claim is
accurate, and the structural pin does pin the actual hazard (a `?` on the
block_on line, or a conditional drop). Defensible.

Close-restructure sanity: `drop(executor)` runs after `block_on` returns, in
sync context — legal for `MemoryTools::Drop`'s `owner.block_on(close)`; the
session's inner `Arc` clone dies with the completed future, so the explicit
drop releases the last reference exactly once; `owner.take()` makes the Drop
idempotent. `main.rs` exits only after `mooshik::run()` returns, so all drops
precede process exit on every path. M4's sync-frame contract preserved; no
double-close.

### P2-d / P2-e — fixed
The widened User set is exactly the 8 variants named in round 1:
`HomeError::{UnsafePath, MigrationRequired, LayoutConflict}`,
`VaultError::{InvalidFormat, UnsafePath, LockFailed, Keyring}`,
`CompanionError::{InvalidResponse, ToolLoop}` — all present in
`is_user_error` **and** all individually asserted in
`exit_codes_distinguish_user_error_from_internal_failure`. Cross-checked the
full enum surfaces: internal set is now `HomeError::Io`,
`VaultError::{Io, Random, KeyDerivation}`, `CompanionError::{Cancelled,
Runtime, Io}`, `MemoryError::Backend` — each genuinely non-reconfiguration.
Tool boundary: `lambo_err` drops the raw display and prints/returns the fixed
fixed-placeholder-free `tools.memory_tool_failed`; the panic catch drops the
payload and prints `tools.tool_panicked`; `panic_message` helper deleted.
Behavioral pin plants DSN material in the error and asserts it never reaches
the result string.

## New-residue hunt

* **SessionConflict payload as conduit** — no. Payload provenance traced into
  lambo source (see P2-c): session id + agent id are operator-authored config
  rendered back to the same operator's own terminal; host/pid/age are OS
  facts. Neither vault values nor credentials can enter this string, and the
  endpoint column is excluded by construction.
* **Widened User set misclassification** — checked the previously-internal
  variants one by one; none has a reconfiguration-style message, so nothing
  that should stay 1 was moved to 2. `VaultError::Keyring` during a headless
  CI run now exits 2 with "Select passphrase mode and provide
  MOOSHIK_VAULT_PASSPHRASE" — that is reconfiguration advice, consistent with
  the documented convention; deliberate pick per round 1's demand, not
  misclassification.
* **Suite count coherence** — full suite: 194 lib/binary tests + 1
  integration test = **195 passed, 0 failed, 1 ignored**, matching the
  remediation's claim (+8 pins over 187).
* **SPEC example loads** — `docs/SPEC.md` contains no runnable command
  examples; the documented examples live in `text/en.toml` help strings and
  are pinned parseable (`every_documented_example_parses_as_written`). Live:
  `init`, `stats`, `recall "deploy checklist"` all exit 0 on a memory-store
  fixture home; `--help` afterword still documents the 0/2/1 convention.
* **File caps** — largest source file is `cli.rs` at 988 lines (< 1000 CI
  limit).
* **en.toml key coverage** — extracted all 92 `text::get("…")` keys used in
  `src/**` and cross-checked against the TOML sections: zero missing keys
  (134 defined). New keys `memory.session_conflict`,
  `tools.memory_tool_failed`, `tools.tool_panicked`,
  `vault.input_too_large` all present; the two tool notices verified
  placeholder-free.
* **Live spot checks** — leading-hyphen name rejected exit 2 with the updated
  `vault.invalid_name` text; `tools.close_failed` now ends with a next step.

## Gates (run once, at end, tree clean)

```
cargo fmt --all -- --check                              clean
cargo clippy --all-targets --locked -- -D warnings      clean
cargo test --locked                                     195 passed · 0 failed · 1 ignored
```

## Verdict

**APPROVE — zero P1/P2 residue found.**

Every round-1 finding's fix was independently re-derived from source,
exercised against the real binary, and attacked with mutations; all five
mutations were caught by the named pins. Remaining nits are P3-grade
(`vault.io_failed` wording serving double duty for IO vs non-UTF-8 stdin;
P3-f TOML-tokenizing still documented-not-changed by design).
