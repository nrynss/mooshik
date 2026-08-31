# M12h — the guided first run: adversarial review, round 4

**APPROVE with zero residue at HEAD 11c35f1**

Reviewer: `M12hReview4` (adversarial pass of the implement → review →
remediate cycle, round 4). Spec: `dev-diary/PLAN.md` "### M12h — what a
first run has to say" (1064–1297) plus the M12h bullet (746–756). The
round-3 remediation record (`m12h-remediation-round3.md`) was checked
against the code, not trusted: every round-3 finding was re-verified at HEAD
by reading the current file, tracing the logic, and re-running the targeted
tests with the environment scrubbed of `MOOSHIK_*`/`LAMBO_*` (plus
`DATABASE_URL` and `GCP_LAMBO_CREDENTIALS`, both ambient). A fresh
adversarial pass then concentrated on the newest remediation code: the
restored `raise`, the handler-body tests, the two-locations trap placement,
`sqlite_at_start` from the file, the restored `mcp_heading`, and the
test-module move. Two live PTY smoke runs exercised the real binary.

Evidence below was produced with read-only checks: `git show`/`git diff`,
targeted `cargo test` runs (env-scrubbed), `cargo build`/`cargo build
--tests`, code tracing, and PTY runs of the built binary against temp homes.
No formatters, linters, or the full suite were run; no files were modified
other than this record. The untracked `ingest-fixtures/workspace/` corpus
(user's mid-flight second-week extension) and the uncommitted
`docs/astro.config.mjs` / `docs/src/content/docs/index.mdx` edits are not
M12h, were not reviewed, and were left untouched.

---

## Round-3 findings: verification at HEAD

### P1 — the restored `raise` — FIXED, live-verified

`src/cli/init_flow.rs:1032-1050` (`restore_echo_and_raise`) now restores the
original termios (1035-1037), sets `signal(signal, SIG_DFL)` (1038), and
`raise(signal)`s (1039) — exactly the round-2 remediation sketch the round-3
review prescribed — before the re-arm block (1043-1048) re-applies no-echo
and re-installs the handler for the fired signal, gated on `ECHO_TERMIOS`
still being `Some`. Terminating signals (SIGINT/SIGQUIT/SIGTERM/SIGHUP) die
inside `raise`; SIGTSTP stops and the re-arm block is reachable again on
resume. Grep confirms no other `raise` and no stale handler body.

**Live PTY verification** (built binary, temp home, passphrase vault,
driven to the "Postgres DSN (not echoed): " prompt):

- Slave termios during the read has ECHO clear (lflag ECHO bit 0); typing
  `postgres://u:p@db/m` is **not** echoed anywhere in the output, and the
  DSN lands in the vault. The no-echo contract is real on a real terminal.
- **Ctrl-C (`\x03`) at the no-echo prompt: the process terminates by
  SIGINT** (signal 2). The terminal's ECHO bit is set again (8) after exit —
  echo restored before death, by construction (tcsetattr runs before raise).
  With the round-3 bug the process kept reading.
- **SIGTERM (`kill`) at the no-echo prompt: the process terminates by
  SIGTERM** (signal 15) with echo restored — the five-signal set works, not
  just SIGINT.
- **Ctrl-Z (`\x1a`): the process remains in state `S`, never `T`** — this
  machine suppresses catchable stops machine-wide (re-confirmed here; the
  round-3 record established it with C/Rust probes, plain, PTY, `setsid`,
  and a `systemd-run --user` scope). The stop/resume path is not observable
  on this host, so the resume re-arm cannot be exercised live here; the
  SIGTSTP round-trip test (below) covers it where stops work, and the
  SIGTERM round trip gates the same `raise` line everywhere.

### The handler-body disposition tests — GENUINE

The two round-3 tests exercise the handler body and fail if the raise is
missing:

- `echo_read_handler_raise_terminates_with_the_default_disposition`
  (`src/cli/init_flow_tests.rs:565-596`): a forked child installs the
  SIGTERM handler, sets `ECHO_TERMIOS`, raises SIGTERM; the handler's
  restore-and-raise kills the child, and the parent asserts
  `WIFSIGNALED` + `WTERMSIG == SIGTERM`. Without the raise the handler
  returns and the child exits via `_exit(2)` — `WIFSIGNALED` fails. The
  remediation record's negative check (raise deleted → test fails) is
  consistent with the code; the assertion is directly on the raise's
  observable effect.
- `echo_read_handler_raises_and_rearms_after_a_stop_resume`
  (`init_flow_tests.rs:478-557`): gated on `sigtstp_stop_is_observable`
  (440-464), which probes whether a default-disposition SIGTSTP actually
  stops a process here; on this machine the probe reports stops are
  suppressed and the test says so instead of misreporting (re-ran with
  `--nocapture`: "note: SIGTSTP stops are suppressed in this environment").
  Where stops work, the parent asserts the child stops (`WIFSTOPPED`,
  `WSTOPSIG == SIGTSTP`), resumes it, and the child asserts the no-echo
  state (`ECHO_TERMIOS` still `Some`) and the re-armed handler
  (`sa_sigaction == restore_echo_and_raise`) after resume, then that the
  guard's drop clears both. `guard.previous[1]` correctly corresponds to
  SIGTSTP in `NO_ECHO_SIGNALS`.
- The install/restore round trip
  (`echo_read_dispositions_are_restored_when_the_guard_drops`, 407-432) is
  unchanged and still covers all five signals returning to their pre-read
  dispositions.

### P2 — the two-locations trap — FIXED

`derive_shared_inference` (`src/cli/init_flow.rs:668-683`) prints
`init.inference_google` as the first line of the `if placeholder_static`
block (673), before the `companion.auth = google` flip and the model set —
exactly when the companion converts to Google from the shipped placeholder
endpoint, and never for a real static endpoint (the `google_location`
derivation outside the gate is the round-2 review's own accepted sketch; it
is inert under static auth). `shared_posture_writes_a_working_config`
asserts the sentence appears (init_flow_tests.rs:145);
`rerun_keeps_a_real_static_endpoint` asserts it does not (277). Both pass.

### P3 — `sqlite_at_start` from the file — FIXED

`src/cli/init_flow.rs:148`:
`let sqlite_at_start = file_value(&source, "store.kind").as_deref() == Some("sqlite");`
— derived from the raw file before any `set` mutates it, so the
`MOOSHIK_STORE_KIND`/`LAMBO_STORE` overlay no longer makes a fresh file
look like a sqlite re-run.
`env_forced_sqlite_still_defaults_a_fresh_embedder_kind_to_bge_m3`
(init_flow_tests.rs:372-400) runs the flow with `MOOSHIK_STORE_KIND=sqlite`
on a fresh file and asserts `Choice [2]:` renders and the written config
lands on `StoreKind::Sqlite` + `EmbedderKind::BgeM3`; the interrupted-choice
case (`local_rerun_kind_default_keeps_an_interrupted_gemini_choice`,
346-370) still asserts `Choice [1]:` with the file genuinely at sqlite.
Both pass.

### P3 — the MCP heading — FIXED

`mcp_step` (`src/cli/init_flow.rs:799`) restores
`self.say(&text::get("init.mcp_heading").replace("{venv}", ...))` right
after the venv file check; `init.mcp_heading` has a call site again, and
`mcp_servers_are_wired_when_the_venv_is_there` asserts the heading appears
(init_flow_tests.rs:209). Passes.

### Test-module move — INTACT

`src/cli/init_flow.rs` is 1116 lines; `src/cli/init_flow_tests.rs` is 611.
Both are under the 1500-line CI cap. A precise `#[test]`-anchored comparison
of the old in-file tests (ad91b20) against HEAD: all 14 old tests present
under `cli::init_flow::init_flow_tests::*`, exactly 3 added (the round-3
handler-body pair plus the env-forced-sqlite test), zero lost, zero
duplicated, zero moved-with-damage (the move's only edits were the new
`drive_env` helper, init_flow_tests.rs:67, and the three new tests).
`cargo test --lib cli::init_flow`: 17 passed, 0 failed.

## Fresh adversarial pass at HEAD: standing contracts

Every standing contract re-checked, all holding:

- **Byte-identical non-TTY contract.** `initialize_unattended`
  (`src/cli/memory_cmd.rs:24-35`) is textually identical to the pre-M12h
  `initialize` body (`git show d702609^:src/cli/memory_cmd.rs`): same
  `layout.init`, `Config::load_at`, `open_vault`, `resolve_secrets`,
  `provision`, and `println!(text::get("home.init_done"))`. The dispatcher
  (memory_cmd.rs:9-18) adds only the `--non-interactive`/`IsTerminal` gate.
- **No new dependencies.** `git diff d702609..HEAD -- Cargo.toml
  Cargo.lock` is empty; libc was already pinned.
- **Strings via `text::get`, no dead keys.** Independent sweep: the `[init]`
  table defines 62 keys, `text::get("init.*")` call sites use 62 — no
  missing, no dead keys (the round-3 record's "62 defined, 62 used" is
  exact). No user-facing string literals in `init_flow.rs` outside
  constants, panic messages, and internal paths.
- **`apply_setting`-only writes.** Every answer goes through
  `config::apply_setting` via `Session::set` (init_flow.rs:271-280). The
  only direct writes are the MCP server blocks through
  `configure::append_mcp_block`/`apply_coder_config`, the documented
  exception (`[mcp_servers.*]` has no settable keys in the `config set`
  allowlist; configure.rs:215-218), which strips any prior same-name block
  and is validated by a `Config::from_toml_and_env` parse before the write
  (init_flow.rs:942-943).
- **Vault-only secrets.** The DSN, credentials path and API key go to the
  vault under fixed names; `config.toml` holds only the secret NAMES and the
  credentials path the plan requires written.
  `secrets_never_appear_in_the_written_file_or_output` passes; the PTY run
  confirmed a typed DSN appears nowhere in the output.
- **Re-runnability.** `rerun_asks_only_for_what_is_still_missing` drives a
  second run with an empty reader and asserts the file is byte-unchanged;
  the interrupted-window and env-forced re-runs are covered above.
- **Parameterized `embedder_question` in every path.** The question renders
  `Choice [1]:` only when `sqlite_at_start && kind == Gemini` (interrupted
  choice), `Choice [2]:` otherwise (fresh local / env-forced sqlite); both
  renderings and all three branches are asserted by tests. The shared
  posture never asks the kind question (embedder_step derives it).
- **`config show`'s missing report** (round-1 P2-3) is still at HEAD:
  `show.rs:271-277` flags the static companion still pointing at the
  placeholder base URL or `local-model`.
- **Prose.** `src/text/en.toml` `[init]` block and `docs/SPEC.md`: no em
  dashes, no semicolons, no sentence over 30 words (scanned), active voice
  throughout. `dev-diary` exempt as specified.
- **Line caps.** 1116 / 611 — both under 1500.
- **Signal-code hygiene.** `read_no_echo` installs the five handlers and
  fills `ECHO_TERMIOS` before echo goes off; `NoEchoRestore::drop` restores
  termios before clearing `ECHO_TERMIOS`; the guard covers every exit path.
  ECHOCTL stays on during the read (only ECHO is cleared), so a typed
  Ctrl-C/Ctrl-Z shows as `^C`/`^Z` — standard for password prompts, and the
  terminal is restored to its original attributes on every path. The
  handler's `tcsetattr`/`signal` calls are the documented, prior-rounds-
  accepted Linux deviation from the POSIX async-signal-safe list (the
  round-2 remediation named it explicitly; the round-3 patch restored
  exactly the review's own sketch).
- **Live behavior.** A wrong DSN is caught at the DSN question: the PTY run
  showed "Store: could not connect or provision" + "Retry? [Y/n] " with a
  fake DSN, exactly the plan's verify-as-you-go contract.

## Findings

None. Every round-3 finding is fixed at HEAD with tests that gate the fix;
the fresh pass found no new defect meeting the review bar (patch-anchored,
provable impact, actionable, unintentional). The residual candidates
(sqlite/bge_m3 retry loops that re-verify the same config until the user
declines — an escape-hatched echo of the plan's own "offer a retry and
allow continuing" requirement; this machine's suppression of catchable
stops) are environment limitations or accepted design, not defects.

## Verification log

Env scrubbed of `MOOSHIK_*`, `LAMBO_*`, `DATABASE_URL`,
`GCP_LAMBO_CREDENTIALS` (ambient on this machine: `LAMBO_POSTGRES_DSN`,
`GCP_LAMBO_CREDENTIALS`):

```
cargo build                          -> Finished (0 warnings)
cargo build --tests                  -> Finished (0 warnings)
cargo test --lib cli::init_flow      -> 17 passed; 0 failed
   (incl. echo_read_handler_raises_and_rearms_after_a_stop_resume,
    echo_read_handler_raise_terminates_with_the_default_disposition,
    env_forced_sqlite_still_defaults_a_fresh_embedder_kind_to_bge_m3)
```

Test-set comparison ad91b20 → HEAD: 14 old tests, 3 added, 0 lost, 0
duplicated. Dead-key sweep: 62 defined / 62 used. Cargo.toml + Cargo.lock:
no diff since d702609. Unattended init body: identical to pre-M12h. Both
`.rs` files under 1500 lines. PTY: ECHO off during secret read, secret
never echoed, Ctrl-C → SIGINT with echo restored, SIGTERM → SIGTERM with
echo restored, Ctrl-Z → state S (environment limitation, documented).
