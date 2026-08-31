# M12h — the guided first run: adversarial review, round 3

**REMEDIATE: 1 P1, 1 P2, 2 P3 at HEAD ad91b20**

Reviewer: `M12hReview3` (adversarial pass of the implement → review →
remediate cycle, round 3). Spec: `dev-diary/PLAN.md` "### M12h — what a
first run has to say" (1064–1297) plus the M12h bullet (746–756). The
remediation-2 record (`m12h-remediation-round2.md`) was checked against the
code, not trusted: every round-2 finding was re-verified at HEAD by reading
the current file, tracing the logic, and re-running the targeted tests with
the environment scrubbed of `MOOSHIK_*`/`LAMBO_*` (plus `DATABASE_URL` and
`GCP_LAMBO_CREDENTIALS`, both ambient). A fresh adversarial pass then
concentrated on the newest remediation code: the re-armed SIGTSTP path, the
five-signal handler set, the install-before-echo-off ordering, the Drop
ordering, `sqlite_at_start`, the parameterized `embedder_question {default}`,
and the disposition test. Two live PTY smoke runs exercised the real binary.

Evidence below was produced with read-only checks: `git show`/`git diff`,
targeted `cargo test` runs (env-scrubbed), `cargo build --tests`, code
tracing, and two PTY runs of the built binary against a temp home. No
formatters, linters, or the full suite were run; no files were modified.

---

## Findings (new or residual, at HEAD)

### P1 — 1. The remediation dropped the `raise`, so every signal is swallowed during a no-echo read and the prompt cannot be interrupted

`src/cli/init_flow.rs:1028-1043` (`restore_echo_and_raise`):

```rust
extern "C" fn restore_echo_and_raise(signal: libc::c_int) {
    unsafe {
        if let Some(original) = ECHO_TERMIOS {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        }
        // Resumed from a stop: the read continues, so the no-echo state and
        // the handler go back in. `ECHO_TERMIOS == None` means the read is over.
        if let Some(original) = ECHO_TERMIOS {
            let mut no_echo = original;
            no_echo.c_lflag &= !libc::ECHO;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &no_echo);
            let _ = install_echo_handler(signal);
        }
    }
}
```

The round-2 P2-2 fix ("re-arm after `raise` returns") was implemented by
**deleting the raise**: ad91b20 removed `libc::signal(signal, libc::SIG_DFL);`
and `libc::raise(signal);` (`git diff 76783f2 ad91b20`, hunk
`@@ -1061,33 +1031,37 @@`) and kept only the block that was supposed to run
*after* the raise returned. There is no `raise` anywhere in the file (grep).
The handler therefore restores echo, immediately re-applies no-echo,
re-installs itself, and returns — the signal is fully consumed. With
SA_RESETHAND the disposition resets to SIG_DFL only at entry; since the
handler never re-raises, the default action never happens.

Consequences, all live-verified on a PTY against the built binary:

* **Ctrl-C at any secret prompt (DSN, credentials path, bearer key, coder
  key) does not abort the run.** The prompt prints `^C` (ECHOCTL) and keeps
  reading. The PTY run sent `\x03` at the "Postgres DSN (not echoed): "
  prompt, then typed a DSN line: the flow stored the DSN and advanced to
  verification. Every other prompt in the flow is abortable with Ctrl-C
  (default disposition); the secret prompts now are not.
* **Ctrl-Z does not suspend the process.** `\x1a` at the same prompt left the
  process in state `S`, not `T` (verified via `/proc/<pid>/stat`), and the
  read continued. The round-2 P2-2 scenario is "fixed" only in the sense that
  the process never stops, so there is never a resume with echo on. The
  re-arm block is unreachable dead logic — the comment "Resumed from a stop"
  cannot be true.
* **SIGTERM/SIGHUP/SIGQUIT are absorbed too.** `kill <pid>` cannot terminate
  the process during a secret read; only `kill -9` (or EOF) works. A terminal
  close (SIGHUP) no longer ends the prompt.

This regresses the 76783f2 behavior (terminate/stop with echo restored) and
defeats the documented purpose of the handler set: the doc comments at
init_flow.rs:970-971, 1002-1004 and 1024-1026 all still promise "re-raise
with the default disposition", which the code no longer does. The round-2
finding's own remediation sketch (restore, `signal(SIG_DFL)`, `raise`, then
re-arm) is the correct shape.

Remediation — restore the raise in front of the re-arm:

```rust
extern "C" fn restore_echo_and_raise(signal: libc::c_int) {
    // Safety: async-signal-safe calls on the process's own stdin.
    unsafe {
        if let Some(original) = ECHO_TERMIOS {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        }
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
        // Resumed from a stop (SIGTSTP): the read continues, so the no-echo
        // state and the handler go back in. `ECHO_TERMIOS == None` means the
        // read is over; terminating signals never get here.
        if let Some(original) = ECHO_TERMIOS {
            let mut no_echo = original;
            no_echo.c_lflag &= !libc::ECHO;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &no_echo);
            let _ = install_echo_handler(signal);
        }
    }
}
```

The disposition test added for P3-5
(`echo_read_dispositions_are_restored_when_the_guard_drops`, init_flow.rs:1457-1484)
only round-trips the install/restore; it never exercises the handler body, so
it cannot catch this. Extend it to cover the raise path (e.g. a
raise-and-resume round trip for SIGTSTP).

### P2 — 2. The plan-mandated two-locations trap statement is no longer printed on the shared posture

`src/cli/init_flow.rs:630-639` (`inference_step`, shared branch): ad91b20
removed `self.say(text::get("init.inference_google"))?;` (hunk
`@@ -654,10 +631,8 @@`). The string survives in `src/text/en.toml:194` but is
now dead (grep: no call site anywhere). Nothing else prints it:
`derive_shared_inference` is silent.

PLAN.md:1237-1243 requires this statement: "The shared posture has a trap in
it that a user cannot guess... companion.google_location = global... the
embedder's location stays us-central1. Those two locations differ and must
differ... first-run should state it rather than leave it to be discovered as
a 404." Rounds 1 and 2 both verified it was printed; 76783f2 kept it. A fresh
shared-posture run at HEAD derives `auth = google`, `google_location = global`
and `model = gemini-3.7-flash` with no sentence explaining why the embedder's
location must stay `us-central1` — a user who "tidies" the two locations into
agreement breaks one side silently, exactly what the plan says must be
prevented. The removal was likely collateral of the P2-1 fix (the old
statement was unconditional and would now be wrong for a real static
endpoint), but the sentence belongs where the conversion actually happens.

Remediation: print the statement when the derivation fires, e.g. inside
`derive_shared_inference`'s `if placeholder_static` block after the
`companion.model` set, or directly before the call in `inference_step` gated
on the placeholder base URL:

```rust
let placeholder_static = base.as_deref() == Some(PLACEHOLDER_BASE_URL);
if placeholder_static {
    self.say(text::get("init.inference_google"))?;
    self.set("companion.auth", "google")?;
    if model.as_deref().is_none_or(|value| value == "local-model") {
        self.set("companion.model", SHARED_MODEL)?;
    }
}
```

### P3 — 3. `sqlite_at_start` reads the resolved store kind, so an env-forced sqlite store makes a fresh run default the embedder to gemini

`src/cli/init_flow.rs:144-146` captures `sqlite_at_start` from the **resolved**
config (`config.store.kind` after the environment overlay), and
`ask_embedder_kind_local` (503-520) makes that the "interrupted choice" flag:
`default_is_gemini = self.sqlite_at_start && kind == Gemini`.

`config::overlay_store_kind` (src/config/overlay.rs:87-98) applies
`MOOSHIK_STORE_KIND` (and `LAMBO_STORE`) over the file. A fresh install ships
`kind = "postgres"` (DEFAULT_TOML, src/config/mod.rs:65) with the shipped
default `kind = "gemini"` embedder. So with `MOOSHIK_STORE_KIND=sqlite` in
the environment — a documented config channel — a **fresh** run resolves as
sqlite at flow start, `sqlite_at_start` is true, and the local kind question
renders `Choice [1]: ` (gemini) instead of the plan's fresh-local bge_m3
default. A plain Enter then silently picks gemini (needs Google credentials;
violates the local posture's "nothing leaving the machine"), and because the
kind lands in the file, every later re-run treats it as a deliberate choice.

This contradicts the remediation record's own documented residual decision
(m12h-remediation-round2.md:95-99): "a file whose store kind is
absent/env-driven at flow start — the kind question still defaults to
bge_m3". The absent case behaves as documented (resolved kind is the postgres
default → bge_m3); the env-driven case does the opposite.

Remediation: derive the flag from the file, not the resolved config:

```rust
// Captured before any `set` mutates the file: sqlite in the FILE here means
// a re-run, whose embedder kind is a choice to keep, not bge_m3 by default.
let sqlite_at_start = file_value(&source, "store.kind").as_deref() == Some("sqlite");
```

### P3 — 4. The MCP heading naming the venv was removed; `init.mcp_heading` is dead

`src/cli/init_flow.rs:793-796` (`mcp_step`): ad91b20 removed
`self.say(&text::get("init.mcp_heading").replace("{venv}", ...))` (hunk
`@@ -819,11 +793,7 @@`). The flow now jumps from the inference step straight
into "Wire up news (web search)? [Y/n] " with no sentence disclosing where
the servers were found; `init.mcp_heading` ("MCP servers: the installer left
them at {venv}.") is defined in en.toml:204 but unused. Minor on its own; the
plan only requires the offer. Restore the line (or delete the key) as part of
the same edit that fixes finding 2, so no dead keys ship.

## Round-2 findings: verification at HEAD

| # | Severity | Finding | Status at HEAD |
|---|---|---|---|
| P2-1 | shared model clobber on a real static endpoint | **Fixed.** Model derivation moved inside `if placeholder_static` (init_flow.rs:670-675); the test seeds `local-model` (1341-1361) and passes. | |
| P2-2 | SIGTSTP resume continues the read with echo on and no handlers | **Not fixed as intended.** The re-arm exists (1036-1041) but the `raise` it was meant to follow was deleted; signals never stop or terminate, so the resume path is unreachable. The failure mode changed from "resume with echo on" to "the process cannot be interrupted or stopped at all" — new P1 #1. | |
| P3-3 | echo-restore holes: unhandled signals, install windows, Drop ordering | **Fixed except where the handler body is broken.** Five-signal set (999-1000), install-before-echo-off (982-991), termios-before-`ECHO_TERMIOS`-clear in Drop (1058-1060) are all correct; but the handler no longer restores-then-raises, so the five-signal coverage is moot (P1 #1). | |
| P3-4 | kind-chosen-before-project window re-asks and defaults to bge_m3 | **Fixed for the named scenario.** `sqlite_at_start` + parameterized default (503-520); test `local_rerun_kind_default_keeps_an_interrupted_gemini_choice` (1427-1452) passes and asserts `Choice [1]:`. Residual edge: env-driven sqlite → gemini default (new P3 #3). | |
| P3-5 | signal code ships with no tests | **Test added** (1457-1484, modeled on tui's precedent, passes) but it exercises only install/restore, not the handler body — it cannot catch P1 #1. | |

## What was checked and passed

- **Round-2 remediation test counts.** `cli::init_flow` 14/14 (12 → 14: the
  kind-default re-run and the disposition test), `config::` 73/73, `text::`
  4/4, `cli::tests` 36/36, `memory::resolve` 7/7 — all env-scrubbed
  (`MOOSHIK_*`, `LAMBO_*`, `DATABASE_URL`, `GCP_LAMBO_CREDENTIALS`);
  `cargo build` clean; `cargo build --tests` 0 warnings.
- **Byte-identical non-TTY contract.** `src/cli/memory_cmd.rs` is
  byte-identical from d702609 through HEAD (`git diff` clean); the dispatcher
  gates on `--non-interactive` + both ttys, and `initialize_unattended` is
  the pre-M12h body verbatim. `--non-interactive` forces it on a real
  terminal.
- **No new dependencies.** `Cargo.toml`/`Cargo.lock` unchanged across the
  whole M12h range (d702609^..ad91b20); `libc` was already pinned. ad91b20
  touches only `init_flow.rs`, `en.toml` and the dev-diary docs.
- **Strings via `text::get`.** All 72 `text::get` call sites in init_flow.rs
  use `init.*` keys present in the `[init]` table; no user-facing literals
  passed directly to `say`/`ask`. Two defined keys are now unused:
  `init.inference_google` and `init.mcp_heading` (findings 2 and 4).
- **Prose rules.** Every `[init]` string and the touched lines
  (en.toml:18 `config.init_after_help`, 43 `config.set_after_help`,
  53 `config.missing_header`, 60 `config.missing_companion_endpoint`,
  113 `memory.missing_dsn`) conform: no em dashes, no semicolons, every
  sentence under 30 words (longest is ~18), active voice. The
  parameterized `embedder_question` (185) renders `Choice [2]:` on a fresh
  local run and `Choice [1]:` on a sqlite re-run with an interrupted gemini
  choice (both asserted by tests); the shared posture never renders it (the
  shared branch goes straight to `shared_google_questions`). SPEC.md after
  the prose pass: 0 em dashes, 0 semicolons. The em-dash/semicolon hits that
  remain in en.toml (`store_move_unconfirmed`, `memory.session_conflict`,
  `reflect`/`companion`/`tools`/`tui` sections, file-header comments) are
  pre-existing and untouched — outside the M12h scope.
- **apply_setting-only writes.** Every answer goes through `Session::set` →
  `config::apply_setting` (validate, 0600, atomic, no symlink) and a reload;
  the MCP blocks use the shared `configure::append_mcp_block` (the one shape
  `config set` cannot write, also used by `configure coder`), with
  `write_source` validating the edited TOML before writing.
- **Vault-only secrets.** DSN, credentials paths, bearer key and coder key
  are read with echo off, stored only in the vault; the file holds names.
  The MCP gate re-stores only configuration values (project id, credentials
  *path*) from config when the vault lacks them — no secret value round-trips
  through config.toml. Scripted tests assert secrets absent from file and
  transcript.
- **Re-runnability.** `rerun_asks_only_for_what_is_still_missing` passes
  (file byte-unchanged on a complete re-run); the differ value is stable;
  store/embedder/inference retries re-ask the likely wrong answer and are
  bounded by user choice.
- **Cross-boundary dispatch.** `mcp_step` → `append_mcp_block` env names
  (`gemini-project`, `gemini-credentials`, `anthropic-api-key`) are
  guaranteed resolvable at spawn by the vault gate/re-store, matching the
  fail-closed `mcp_host` resolution.
- **PTY smoke runs (live binary, temp home, passphrase vault).** (1) Ctrl-C
  at the "Postgres DSN (not echoed): " prompt: the flow kept reading, stored
  the DSN, and advanced — the signal was swallowed (evidence for P1 #1). (2)
  Ctrl-Z at the same prompt: process state stayed `S` (never stopped) and the
  read continued. Both runs cleaned up (SIGKILL, temp homes removed).
- **Line cap (risk, not a defect).** CI enforces `wc -l > 1500` per `.rs`
  (ci.yml:55-62); `init_flow.rs` is exactly 1500 lines, so HEAD passes but
  any future edit to the file breaks CI. Recommend trimming now while the
  P1/P2 fixes land: move the `#[cfg(test)] mod tests` block (lines 1107-1500)
  into a sibling `init_flow/tests.rs` — the flow's test module is self-
  contained and needs no `pub` surface changes — or split the module. Do not
  treat being at the cap as a defect in itself.

## Verdict

**REMEDIATE: 1 P1, 1 P2, 2 P3 at HEAD ad91b20.** The round-2 remediation
fixed the model clobber, the Drop ordering, the install windows, the
five-signal coverage and the kind-default window as documented, but the P2-2
"re-arm" fix deleted the `raise` itself, so the whole handler set now
swallows every signal and the no-echo reads cannot be interrupted, stopped,
or killed by any of the five signals (live-verified). The same commit also
silently removed the plan-mandated two-locations trap statement and the MCP
heading, leaving two dead en.toml keys, and the new `sqlite_at_start` flag
misreads an env-forced sqlite store as a re-run. All four are patch-anchored
to ad91b20.
