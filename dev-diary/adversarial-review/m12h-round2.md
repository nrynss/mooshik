# M12h — the guided first run: adversarial review, round 2

**REMEDIATE: 2 P2, 3 P3 at HEAD 1a5039e**

Reviewer: `M12hReview2` (adversarial pass of the implement → review → remediate
cycle, round 2). Spec: `dev-diary/PLAN.md` "### M12h — what a first run has to
say" (1064–1297) plus the M12h bullet (746–756). The remediation record
(`m12h-remediation-round1.md`) was checked against the code, not trusted:
every one of the 8 round-1 findings was re-verified at HEAD by reading the
current file (not the diff), tracing the logic, and re-running the targeted
tests with the environment scrubbed of `MOOSHIK_*`/`LAMBO_*`. The review then
took a fresh adversarial pass over the whole milestone, concentrating on the
new remediation code: the differ-offer, the vault-gated MCP offer, the
NoEchoRestore signal handling, the retry re-ask, and the P2-6 skip logic.

Evidence below was produced with read-only checks: `git show`/`git diff`,
targeted `cargo test` runs (env-scrubbed), `cargo build`, and code tracing.
No formatters, linters, or the full suite were run; no files were modified.

---

## Findings (new or residual, at HEAD)

### P2 — 1. `derive_shared_inference` still clobbers the model when the endpoint is real

`src/cli/init_flow.rs:694-703`:

```rust
let placeholder_static = base.as_deref() == Some(PLACEHOLDER_BASE_URL);
if placeholder_static {
    self.set("companion.auth", "google")?;
}
if !file_has(&self.source, "companion.google_location") {
    self.set("companion.google_location", "global")?;
}
if model.as_deref().is_none_or(|value| value == "local-model") {
    self.set("companion.model", SHARED_MODEL)?;
}
```

The P2-1 fix stopped the `auth` flip for a real static endpoint, but the model
derivation is not gated on `placeholder_static` — it fires whenever the file
carries `local-model`, the shipped default (`src/config/mod.rs:82`). The exact
P2-1 scenario therefore ends in an inconsistent combo: fresh install (model
`local-model`) + `mooshik config set companion.base_url https://my-llm.example/v1`
+ shared-posture `init` re-run → `auth` stays `static` (fixed), but `model`
becomes `gemini-3.7-flash` while `base_url` still points at the user's endpoint.
Chat then POSTs `{"model": "gemini-3.7-flash"}` to a local OpenAI-compatible
server that serves no such model. Nothing in the flow can repair it: the
inference retry for `Static` auth re-asks only the base URL (init_flow.rs:778-789),
`ask_inference_local` (the only place the model is asked) is unreachable on the
shared posture (init_flow.rs:680-683), and `config show`'s missing report
(`src/config/show.rs:272-277`) flags neither `base_url` (real) nor `model`
(no longer `local-model`). Pre-remediation the same file state was converted
fully to Google (consistent, though wrong); the remediation's partial fix
manufactures the inconsistent state. The remediation's own test
`rerun_keeps_a_real_static_endpoint` seeds `model = "gemini-3.7-flash"`
(init_flow.rs:1395), the one value that makes its `model` assertion hold.

Remediation: derive the shared model (and arguably `google_location`) only when
the companion is being converted to Google — move the model line inside
`if placeholder_static`:

```rust
let placeholder_static = base.as_deref() == Some(PLACEHOLDER_BASE_URL);
if placeholder_static {
    self.set("companion.auth", "google")?;
    if model.as_deref().is_none_or(|value| value == "local-model") {
        self.set("companion.model", SHARED_MODEL)?;
    }
}
if !file_has(&self.source, "companion.google_location") {
    self.set("companion.google_location", "global")?;
}
```

### P2 — 2. SIGTSTP resume continues the no-echo read with echo on and no handlers

`src/cli/init_flow.rs:1058-1067` (`restore_echo_and_raise`):

```rust
if let Some(original) = ECHO_TERMIOS {
    libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
}
libc::signal(signal, libc::SIG_DFL);
libc::raise(signal);
```

For SIGTSTP this restores the **original** termios (echo on), sets SIG_DFL, and
raises — the process stops. When the user runs `fg`, execution resumes inside
`raise`, the handler returns, and `read_plain_line` continues with echo **on**
and the SIGINT/SIGTSTP dispositions at SIG_DFL (SA_RESETHAND reset them on
entry; the explicit `signal(SIG_DFL)` confirms). The guard is still alive but
does nothing until the read completes. The user — still at the secret prompt —
keeps typing the rest of the DSN/credential path/key, and it is echoed to the
terminal and remains in scrollback after the process exits. That breaks the
plan's contract ("A DSN or a credential path is never echoed", PLAN.md:1081-1084)
in exactly the interrupt-and-resume path the handler was built for. A second
Ctrl-C in the resumed read also terminates outright (echo on, so the terminal
itself is fine — but the no-echo protection is gone for the remainder of the
read either way).

Remediation: re-arm after the stop — once `raise` returns (i.e. after `fg`),
re-apply the no-echo attributes and re-install the handler:

```rust
extern "C" fn restore_echo_and_raise(signal: libc::c_int) {
    // Safety: async-signal-safe calls on the process's own stdin.
    unsafe {
        if let Some(original) = ECHO_TERMIOS {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        }
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
        // Resumed (SIGCONT): the read continues, so the no-echo state and
        // the handler must be back in place for the rest of the secret.
        if let Some(original) = ECHO_TERMIOS {
            let mut no_echo = original;
            no_echo.c_lflag &= !libc::ECHO;
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &no_echo);
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = restore_echo_and_raise as *const () as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            action.sa_flags = libc::SA_RESETHAND | libc::SA_NODEFER;
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
}
```

### P3 — 3. Echo restore still has holes: unhandled signals, and two narrow windows in the new code

`src/cli/init_flow.rs:1021-1026` and `1084-1085`:

```rust
ECHO_TERMIOS = Some(original);
let mut guard = NoEchoRestore { ... };
guard.sigint = Some(install_echo_handler(libc::SIGINT)?);
guard.sigtstp = Some(install_echo_handler(libc::SIGTSTP)?);
...
ECHO_TERMIOS = None;
libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.termios);
```

Three residual gaps in the "echo cannot be left off" guarantee the remediation
claims (its own doc: "Ctrl-C/Ctrl-Z cannot leave it with echo off"):

1. Only SIGINT and SIGTSTP get the restore-and-raise handler. SIGQUIT (Ctrl-\),
   SIGTERM (`kill`), and SIGHUP during a secret prompt still terminate with
   their default dispositions and the final `tcsetattr` never runs — the
   round-1 finding's class ("terminal left with echo disabled") survives for
   every signal but the two named. The same handler works for them unchanged.
2. The window between `tcsetattr(no-echo)` (line 1018) and the handler installs
   (lines 1025-1026): a signal delivered there terminates with echo off.
3. In `Drop`, `ECHO_TERMIOS = None` runs *before* the termios restore (1084 →
   1085) while the handlers are still installed: a signal in that window runs
   `restore_echo_and_raise`, which finds `ECHO_TERMIOS == None`, skips the
   restore, and re-raises to termination with echo still off.

Windows 2 and 3 are sub-microsecond; the unhandled signals are user-reachable.
Remediation: extend `install_echo_handler` to SIGQUIT/SIGTERM/SIGHUP as well;
in `Drop`, restore the termios before clearing `ECHO_TERMIOS`.

### P3 — 4. P2-6 residual: kind-chosen-before-project interrupted window still re-asks the kind question

`src/cli/init_flow.rs:487-496` (`embedder_step`, local branch):

```rust
if file_has(&self.source, "embedder.gemini_project")
    || file_has(&self.source, "embedder.gemini_credentials")
{
    self.shared_google_questions()?;
    return self.verify_embedder();
}
self.ask_embedder_kind_local()?;
```

The skip fires only when a project or credentials key exists. The window
between `set("embedder.kind", "gemini")` and the project answer
(`ask_embedder_kind_local`, line 533 → `shared_google_questions`) leaves
`kind = "gemini"` in the file with no project/credentials key — byte-identical
to a fresh default file, which also ships `kind = "gemini"`
(`src/config/mod.rs:68`). An interruption in that window makes the re-run fall
through to `ask_embedder_kind_local`, whose default is bge_m3 (`"" | "2"` →
`set("embedder.kind", "bge_m3")`), silently replacing the user's gemini choice
— the same clobber the round-1 finding described, one keystroke earlier in the
interrupted run. This is inherent to the file-state heuristic (the round-1
prescription "skip when kind == Gemini" would regress the fresh-run kind
question, which must still be asked because the shipped default kind is
gemini); the named round-1 scenario (project entered) is fixed. Flagged so the
residual is on the record and a decision is explicit rather than implicit.

### P3 — 5. The new signal-handling code ships with no tests, unlike the repo's own precedent

`src/cli/init_flow.rs:1036-1045` (`install_echo_handler`) through
`1073-1094` (`NoEchoRestore`) are entirely untested — the remediation record
concedes "verified by code inspection and build only". The repo's own
precedent, `src/tui/mod.rs:479-497`
(`a_termination_signal_disposition_is_restored_after_the_session`), shows the
established standard for signal code: a disposition round-trip test that reads
`sigaction` before and after, asserting the session's handler is not left
installed. `install_echo_handler` and `NoEchoRestore::drop` need no tty — they
are pure `sigaction` installs/restores — so the same shape of test is
writable here and would have caught regressions in findings 2 and 3 (e.g. the
Drop ordering). Add it.

---

## Round-1 findings: verification at HEAD

| # | Severity | Finding | Status at HEAD |
|---|---|---|---|
| P2-1 | real static endpoint clobbered when `auth` absent | **Fixed** for `auth`/`base_url` (placeholder now gated on the base URL alone, init_flow.rs:694); residual model clobber → new P2 #1 | |
| P2-2 | no offer to differ for the cloud project | **Fixed** (init_flow.rs:579-613). Plain Enter keeps the same project (`ask_yes` default true; empty differ answer → `project.clone()`). An existing `companion.google_project` is never overwritten — the offer sits inside `if companion_project_missing`. Local posture never offers it (`matches!(store.kind, Postgres\|Cockroach)`). Vault keeps the embedder's project, which is what the MCP env map names — correct for the differ case (test asserts it). | |
| P2-3 | `config show` missing report omits placeholder companion | **Fixed** (show.rs:272-277); bullets fire for shipped default, not for a real static endpoint; tests added and passing. | |
| P2-4 | news/artifacts offered when vault lacks gemini secrets | **Fixed** (init_flow.rs:827-855). Gate is now `vault().get("gemini-project") && vault().get("gemini-credentials")`, with a re-store from config when missing. **The re-stored values are the project id and the credentials *path* — both are configuration the file legitimately carries (the plan says the path is config); no secret value is ever read back out of `config.toml`.** The DSN/API keys, the actual secrets, are never re-stored (they are not in the config). | |
| P2-5 | echo not restored when a secret read is interrupted | **Fixed for SIGINT/SIGTSTP** (guard + handler); residuals → new P2 #2 and P3 #3. | |
| P2-6 | local re-run re-asks kind and defaults a chosen gemini to bge_m3 | **Fixed for the named scenario** (project/credentials key present); residual window → P3 #4. Fresh first run still asks the kind question (verified: shipped default kind is gemini and neither key is in the file, so `file_has` is false); only the file-carries-gemini case skips. | |
| P3-7 | google inference retry re-runs without re-asking | **Fixed** (init_flow.rs:790-801): re-asks the credentials path for non-Static auth, writes both keys + vault. Retry is bounded by `ask_yes` — no infinite loop. | |
| P3-8 | unused `config` binding in `read_vault` | **Fixed** (init_flow.rs:1212-1213); `cargo build --tests` clean. | |

## What was checked and passed

- **Byte-identical non-TTY contract.** `src/cli/memory_cmd.rs` at HEAD is
  byte-identical to d702609 (the remediation commit did not touch it);
  `initialize_unattended` (memory_cmd.rs:24-35) is the pre-M12h `initialize`
  body verbatim (`git show d702609^:src/cli/memory_cmd.rs` diffed clean apart
  from the wrapper split).
- **Build and targeted tests (env scrubbed of `MOOSHIK_*`/`LAMBO_*`):**
  `cargo build` clean; `cli::init_flow` 12/12 (7 pre-existing updated for the
  differ line + 5 new); `config::` 73/73 (2 new in show.rs).
- **Line cap.** CI enforces 1500 lines per `.rs` (ci.yml:55-57);
  `init_flow.rs` is 1493, `show.rs` 422.
- **String discipline.** New strings `init.inference_same_project`,
  `init.inference_differ_project`, `config.missing_companion_endpoint` all via
  `text::get` and present in en.toml; every `text::get("init.*")` call site
  cross-checked against the `[init]` table — no missing keys.
- **Differ-offer specifics.** Enter keeps the same project; an existing
  `companion.google_project` is never clobbered; the offer fires only on the
  shared posture and only when the companion side was actually being filled.
- **Vault-restore specifics.** Re-stores only non-secret configuration (project
  id, credentials path); secret values never round-trip through config.
- **Retry re-ask.** Bounded by user choice in every loop
  (`verify_store`/`verify_embedder`/`verify_inference`); Google inference retry
  now re-asks the credentials (the likely wrong answer).
- **SA_RESETHAND / SA_NODEFER mechanics.** Correct: the disposition is reset on
  entry so the handler's `raise` takes the default action; SA_NODEFER unblocks
  the re-raise; `signal`/`raise` are async-signal-safe. `tcsetattr` from a
  handler is not on POSIX's async-signal-safe list but is a single ioctl on
  Linux; noted as a mild deviation from the repo's tui precedent, which limits
  its handler to a relaxed atomic store — acceptable, and the basis for
  findings P2 #2 / P3 #3 rather than a hard async-safety defect.
- **Re-runnability.** `rerun_asks_only_for_what_is_still_missing` still passes
  (file byte-unchanged on a complete re-run); the differ value is stable across
  re-runs; the store's missing-vault-reference case re-asks correctly.
- **Cross-boundary dispatch.** `mcp_step` → `append_mcp_block` env names
  (`gemini-project`, `gemini-credentials`) now guaranteed resolvable at spawn
  by the vault gate/re-store; coder path unchanged and still re-runnable.
- **Plan-point coverage** (from round 1, re-checked against the current code):
  unchanged by the remediation; posture-first with shared default, store
  branches, sticky embedder warning, once-each project/credentials, derived
  inference with the two-locations trap, verify-each-answer with
  retry-or-continue and a closing unverified list, MCP offer gated on the venv
  with one-keystroke decline, `mooshik tui` closing advice. `memory` and
  `fixture` never offered.
