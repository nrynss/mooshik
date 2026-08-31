# M12h — the guided first run: remediation record, round 3

Remediation of the 4 findings (1 P1, 1 P2, 2 P3) in
`dev-diary/adversarial-review/m12h-round3.md`, at HEAD `ad91b20`
(branch `main`). `local://m12h-remediation-3.md` was unavailable; the review
file was authoritative. No formatters, linters, or the full suite were run;
nothing was committed. `mcp-servers` venv trees and `target/` untouched.

---

## Findings remediated

### P1 — 1. The remediation dropped the `raise`, so every signal was swallowed during a no-echo read — FIXED

**Changed** (`src/cli/init_flow.rs:1032-1050`): `restore_echo_and_raise`
again restores the original termios, sets `signal(signal, SIG_DFL)`, and
`raise(signal)`s — exactly the round-2 remediation sketch
(`src/cli/init_flow.rs:1038-1039`) — *before* the re-arm block. The handler
now restores echo and lets the default disposition act: SIGINT/SIGQUIT/
SIGTERM/SIGHUP terminate the no-echo read, SIGTSTP stops it, and the
re-arm block (re-apply no-echo, re-install the handler, gated on
`ECHO_TERMIOS` still being `Some`) is reachable again for the stop/resume
path. The doc comments (init_flow.rs:1028-1030) that promise the re-raise
are true again.

**Test extended** (`src/cli/init_flow_tests.rs:440-471, 478-561, 565-594`):

1. `echo_read_handler_raises_and_rearms_after_a_stop_resume`
   (init_flow_tests.rs:478): a forked child installs the SIGTSTP handler,
   sets `ECHO_TERMIOS` and raises SIGTSTP; the parent observes the stop
   (`WIFSTOPPED` + `WSTOPSIG == SIGTSTP`), resumes with SIGCONT, and the
   child asserts the no-echo state (`ECHO_TERMIOS` still `Some`) and the
   handler are back in place after resume, then that dropping the guard
   restores the dispositions and clears `ECHO_TERMIOS`. **It fails if the
   raise is missing**: the child then never stops and the parent's
   `stopped` assertion fires. Where the sandbox suppresses catchable stops
   (see PTY note below), `sigtstp_stop_is_observable` (init_flow_tests.rs:
   440) probes it and the test says so instead of misreporting.
2. `echo_read_handler_raise_terminates_with_the_default_disposition`
   (init_flow_tests.rs:565): the same raise line through the terminate
   class, which every environment supports. A child installs the SIGTERM
   handler and raises it; the re-raised default action kills the child by
   SIGTERM. **Verified to fail with the raise deleted**: temporarily
   removing `signal`/`raise` made it panic "child was not terminated by the
   re-raised signal: the raise is missing" (then restored).
3. The install/restore round trip
   (`echo_read_dispositions_are_restored_when_the_guard_drops`,
   init_flow_tests.rs:407) is unchanged.

**Evidence**: negative check — with the two lines deleted, the SIGTERM
round trip fails; restored, `cli::init_flow` 17/17 pass. Live PTY check
below confirms Ctrl-C now aborts a no-echo read.

### P2 — 2. The plan-mandated two-locations trap statement is printed again, exactly when the derivation fires — FIXED

**Changed** (`src/cli/init_flow.rs:671`): `derive_shared_inference` prints
`text::get("init.inference_google")` as the first line of the
`if placeholder_static` block (before the `companion.auth` flip and the
model set), so the statement renders precisely when the companion is being
converted to Google from the shipped placeholder endpoint, and never for a
real static endpoint. `init.inference_google` (en.toml:194) has a call
site again.

**Test** (`src/cli/init_flow_tests.rs:144-146, 273-277`):
`shared_posture_writes_a_working_config` now asserts the output contains
"Inference: Vertex Gemini" (the shared fresh run must state the trap);
`rerun_keeps_a_real_static_endpoint` asserts it does **not** appear for a
real static endpoint.

**Evidence**: both tests pass; the dead-key sweep (below) confirms the key
is used.

### P3 — 3. `sqlite_at_start` now reads the FILE, so an env-forced sqlite store still defaults the local kind to bge_m3 — FIXED

**Changed** (`src/cli/init_flow.rs:148`): the flag is derived from the raw
file before any `set` mutates it —

```rust
let sqlite_at_start = file_value(&source, "store.kind").as_deref() == Some("sqlite");
```

— instead of from the resolved `config.store.kind`, so the
`MOOSHIK_STORE_KIND` (and `LAMBO_STORE`) overlay no longer makes a fresh
file look like a sqlite re-run. A fresh install ships `kind = "postgres"`
in the file, so the env-forced case now keeps the fresh-local bge_m3
default (`Choice [2]:`); a file that genuinely says sqlite still keeps an
interrupted gemini choice (`Choice [1]:`, covered by the existing
`local_rerun_kind_default_keeps_an_interrupted_gemini_choice`).

**Test added** (`src/cli/init_flow_tests.rs:372-403`):
`env_forced_sqlite_still_defaults_a_fresh_embedder_kind_to_bge_m3` runs the
flow with `MOOSHIK_STORE_KIND=sqlite` in the environment (via the new
`drive_env` helper, init_flow_tests.rs:67) on a fresh file and asserts
`Choice [2]:` renders and the written config lands on `StoreKind::Sqlite` +
`EmbedderKind::BgeM3`. **Verified to fail with the resolved-config
derivation restored** (the kind question then renders `Choice [1]:` and a
plain Enter picks gemini); the file-based line passes it.

**Evidence**: negative check + 17/17 green.

### P3 — 4. The MCP heading naming the venv is printed again — FIXED

**Changed** (`src/cli/init_flow.rs:799`): `mcp_step` restores
`self.say(&text::get("init.mcp_heading").replace("{venv}", ...))` right
after the venv file check, so the flow discloses where the installer left
the servers before offering to wire them. `init.mcp_heading` (en.toml:204)
has a call site again.

**Test** (`src/cli/init_flow_tests.rs:208-210`):
`mcp_servers_are_wired_when_the_venv_is_there` asserts the output contains
"MCP servers: the installer left them at".

**Evidence**: test passes; dead-key sweep below confirms the key is used.

---

## Line cap

`src/cli/init_flow.rs` dropped from exactly 1500 lines to **1116** by
moving the whole `#[cfg(test)] mod tests` block into a sibling file
(`src/cli/init_flow.rs:1114-1116` declares
`#[cfg(test)] #[path = "init_flow_tests.rs"] mod init_flow_tests;`).
`src/cli/init_flow_tests.rs` is **611** lines. Both are comfortably under
the 1500-line CI cap. No `pub` surface changed; test paths are now
`cli::init_flow::init_flow_tests::*`.

## Dead en.toml keys

Swept every `text::get("init.*")` call site in `src/**/*.rs` against the
`[init]` table: **62 defined, 62 used — no dead keys ship.** The two keys
the review flagged (`init.inference_google`, `init.mcp_heading`) are used
again by findings 2 and 4. No keys were removed (none were dead).

## PTY verification of the P1 fix

Live run of the built binary (temp home, passphrase vault, `mooshik init`)
on a real PTY, driving to the "Postgres DSN (not echoed): " prompt:

- **Ctrl-C (`\x03`) at the no-echo prompt: the process terminates.** State
  went `S` → `Z` (reaped), i.e. SIGINT's default action ran after the
  handler restored the termios — the read is aborted. With the round-3 bug
  the process kept reading (the review's evidence). Echo restoration before
  death is by construction: `tcsetattr` runs before `raise` in the handler.
- **Ctrl-Z (`\x1a`) stop/resume is not observable on this machine.**
  Machine-wide, the default SIGTSTP action leaves a process in state `S`,
  never `T`: verified with minimal C and Rust probes (plain, PTY,
  `setsid`, and a fresh `systemd-run --user` scope) — SIGSTOP (uncatchable)
  stops normally and SIGINT/SIGTERM default actions terminate normally, but
  the catchable-stop path never stops. So the resume branch of
  `restore_echo_and_raise` cannot be exercised in-suite here; the SIGTERM
  round trip verifies the same `raise` line instead, and the SIGTSTP round
  trip asserts the full stop/resume/re-arm cycle on any environment where
  stops work (the test probes and reports when they do not).

## Verification

Env scrubbed of `MOOSHIK_*`, `LAMBO_*`, `DATABASE_URL`,
`GCP_LAMBO_CREDENTIALS` (ambient on this machine: `LAMBO_POSTGRES_DSN`,
`GCP_LAMBO_CREDENTIALS`):

```
cargo build           -> Finished (0 warnings)
cargo build --tests   -> Finished (0 warnings)
cargo test --lib cli::init_flow  -> 17 passed; 0 failed   (was 14; +3: env-forced
                                        sqlite default, SIGTSTP stop/resume round
                                        trip, SIGTERM raise round trip)
cargo test --lib config::         -> 73 passed; 0 failed
cargo test --lib text::           -> 4 passed; 0 failed
cargo test --lib cli::tests       -> 36 passed; 0 failed
cargo test --lib memory::resolve  -> 7 passed; 0 failed
```

Negative checks (temporarily reverting, then restoring): removing the
`signal`/`raise` lines fails the SIGTERM round trip; restoring
resolved-config `sqlite_at_start` fails the env-forced test.

## Confirmation

The `raise` is restored: `src/cli/init_flow.rs:1038-1039` —
`libc::signal(signal, libc::SIG_DFL);` then `libc::raise(signal);` sit in
front of the re-arm block, exactly per the review's code sketch, and the
disposition tests now exercise the handler body (both round trips fail if
the raise is missing). All 4 findings remediated; every targeted test
passes; `init_flow.rs` is 1116 lines; no dead en.toml keys; nothing
committed.
