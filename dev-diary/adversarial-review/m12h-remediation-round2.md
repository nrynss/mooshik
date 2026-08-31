# M12h — the guided first run: remediation record, round 2

Remediation of the 5 findings (2 P2, 3 P3) in
`dev-diary/adversarial-review/m12h-round2.md`, at HEAD `76783f2`
(branch `main`). `local://m12h-remediation-2.md` was unavailable; the review
file was authoritative. No formatters, linters, or the full suite were run;
nothing was committed.

---

## Findings remediated

### P2 — 1. `derive_shared_inference` clobbers the model when the endpoint is real — FIXED

**Changed** (`src/cli/init_flow.rs:666-683`): the `companion.model`
derivation moved inside the `if placeholder_static` guard, alongside the
`companion.auth` flip, so the shared model (`gemini-3.7-flash`) is derived
only when the companion is being converted to Google. A real `base_url` set
with `config set` now keeps its static auth **and** its model — including
the shipped `local-model` default, which a user endpoint does not serve as
`gemini-3.7-flash`.

**Test updated** (`src/cli/init_flow.rs:1341-1361`):
`rerun_keeps_a_real_static_endpoint` now seeds `model = "local-model"` (the
shipped default, previously seeded as `gemini-3.7-flash` — the one value
that masked the bug) and asserts `config.companion.model == "local-model"`
plus `!written.contains("gemini-3.7-flash")`, along with the existing
static-auth assertions.

**Evidence**: the old code rewrote `local-model` → `gemini-3.7-flash` on the
real-endpoint re-run (test failed); with the fix the model survives
(`cli::init_flow` 14/14 pass).

### P2 — 2. SIGTSTP resume continues the no-echo read with echo on and no handler — FIXED

**Changed** (`src/cli/init_flow.rs:1028-1045`): `restore_echo_and_raise` now
re-arms after `raise` returns (i.e. after `fg` resumes a stopped SIGTSTP
read): it re-applies the no-echo termios and re-installs the
restore-and-raise handler for the signal that fired, gated on
`ECHO_TERMIOS` still being `Some` (the guard dropping meanwhile means the
read is over — nothing to re-arm). The rest of the secret is again read
with echo off and with the SIGINT/SIGTSTP/SIGQUIT/SIGTERM/SIGHUP
dispositions covered. The re-install reuses `install_echo_handler`
(`sigaction`/`sigemptyset` are async-signal-safe; only `tcsetattr`, the
already-accepted Linux deviation, is not on POSIX's list).

**Evidence**: code tracing of the stop/resume path; the disposition
round-trip test (P3-5) covers the install/restore pair.

### P3 — 3. Echo restore holes: unhandled signals, install windows, Drop ordering — FIXED

**Changed** (`src/cli/init_flow.rs:999-1000, 973-997, 1048-1066`):

1. The handler set grew from SIGINT/SIGTSTP to the full
   `NO_ECHO_SIGNALS = [SIGINT, SIGTSTP, SIGQUIT, SIGTERM, SIGHUP]`
   (init_flow.rs:999-1000): SIGQUIT (Ctrl-\), SIGTERM (`kill`) and SIGHUP
   during a secret prompt now restore the terminal and re-raise with the
   default disposition instead of dying with echo off.
2. Install windows closed: in `read_no_echo` the handlers are installed
   **before** echo comes off and `ECHO_TERMIOS` is filled **before** the
   `tcsetattr(no-echo)` (init_flow.rs:981-992), so no signal in the install
   sequence can die with echo off; the handler already tolerates
   `ECHO_TERMIOS == None` (init_flow.rs:1031). A `tcsetattr` failure drops
   the guard, which restores the dispositions and the still-on echo.
3. `NoEchoRestore::drop` now restores the termios **before** clearing
   `ECHO_TERMIOS` (init_flow.rs:1056-1058), so a signal in the remaining
   window still finds the saved attributes and restores echo before dying.

**Evidence**: the disposition round-trip test (P3-5) asserts all five
signals return to their pre-read disposition after the guard drops; the
`ECHO_TERMIOS`-before-clear ordering is enforced in Drop and covered by
code tracing.

### P3 — 4. Kind-chosen-before-project window still re-asks and defaults to bge_m3 — FIXED

**Changed** (`src/cli/init_flow.rs:503-530`, `193`, `150-155`): the local
kind re-ask now defaults to the file's current embedder kind when the store
was **already sqlite at flow start** (`Session::sqlite_at_start`, captured
in `run_with` before any `set` mutates the file), else bge_m3. Reaching the
question with `kind = gemini` in the file on a sqlite store therefore means
an interrupted choice, and a plain Enter keeps gemini (matching the prompt,
whose default is now parameterized: `src/text/en.toml:185`
`embedder_question` uses `{default}`, house-prose-conforming).

**Test added** (`src/cli/init_flow.rs:1427-1455`):
`local_rerun_kind_default_keeps_an_interrupted_gemini_choice` seeds the
exact window state (sqlite store, `kind = "gemini"`, no project/credentials
key), re-runs with an Enter as the kind answer, and asserts the kind stays
gemini, the project/credentials fill, `!written.contains("bge_m3")`, and
the prompt showed `Choice [1]:`.

**Residual decision (explicit)**: the file-state heuristic remains the
source of truth, and a re-run can only preserve a gemini kind when the file
shows the sqlite store the local posture implies. Outside that window —
a store switched to sqlite within the same run, or a file whose store kind
is absent/env-driven at flow start — the kind question still defaults to
bge_m3, because the file carries no marker distinguishing "interrupted
gemini choice" from the shipped default gemini of a fresh shared file, and
the fresh local default must stay bge_m3. The named round-1 scenario and
this window are both fixed; the store-switch edge is accepted by design.

### P3 — 5. Signal-handling code ships with no tests — FIXED

**Test added** (`src/cli/init_flow.rs:1457-1485`):
`echo_read_dispositions_are_restored_when_the_guard_drops`, modeled on
`src/tui/mod.rs:479-497`: for every signal in `NO_ECHO_SIGNALS` it reads
`sigaction` before, installs via `install_echo_handler`, drops a
`NoEchoRestore`, and reads `sigaction` after, asserting the handler is not
left installed. No tty is needed (`install_echo_handler`/`drop` are pure
`sigaction` installs/restores; the guard's `tcsetattr` is a no-op on a
non-tty stdin, and the test reads the real termios first so a tty-stdin
run is harmless). `#[cfg(unix)]` as in the tui precedent.

**Evidence**: `cli::init_flow` runs it (14/14 total).

---

## Verification (env scrubbed of `MOOSHIK_*` and `LAMBO_*`)

Commands and results, all from `/home/nryn/work/mooshik` with
`env -u LAMBO_POSTGRES_DSN` (the only ambient `MOOSHIK_*`/`LAMBO_*`
variable; it causes the pre-existing `moving_the_store_is_refused` failure,
so it is scrubbed):

```
cargo build                                          -> clean
cargo build --tests                                  -> 0 warnings
cargo test --lib cli::init_flow                      -> 14 passed, 0 failed
cargo test --lib config::                            -> 73 passed, 0 failed
cargo test --lib text::                              -> 4 passed, 0 failed
cargo test --lib cli::tests                          -> 36 passed, 0 failed
cargo test --lib memory::resolve                     -> 7 passed, 0 failed
```

Test counts: `cli::init_flow` 12 → 14 (+2: P3-4 kind-default re-run,
P3-5 disposition round-trip); all other suites unchanged.

## Constraints

- `init_flow.rs` is **1500 lines** (CI cap is `> 1500`, ci.yml:55-57);
  `src/text/en.toml` untouched except the `embedder_question` `{default}`
  parameterization (line 185).
- New strings: none added (the existing `embedder_question` was
  parameterized, no em dash, no semicolon, under 30 words). `text::get`
  only; config writes via `apply_setting` only; secrets vault-only; the
  non-TTY init path is untouched.
- No commit; no formatters, linters, or full suite; `target/` and
  `mcp-servers` venv trees untouched.
