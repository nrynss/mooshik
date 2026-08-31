# M12h — remediation, round 1 (8/8 findings)

Remediated: all 8 findings (6 P2, 2 P3) from `dev-diary/adversarial-review/m12h-round1.md`,
at HEAD 0b96d3b. The remediation plan file (`local://m12h-remediation-1.md`) was not
visible to the worker, so the review file was the authoritative finding list.

Changes: `src/cli/init_flow.rs`, `src/config/show.rs`, `src/text/en.toml`, plus the
scripted tests in `init_flow.rs`. No new dependencies; all new strings via `text::get`;
config writes stay on `config::write::apply_setting`; secrets remain vault-only with echo
off; the non-TTY/`--non-interactive` path is untouched (no edits to `memory_cmd` or the
unattended body). Nothing committed; no formatters/linters/full suite run.

---

## Per finding

### P2-1 — Don't clobber a real static endpoint when `companion.auth` is absent

`derive_shared_inference` now treats the file as the shipped placeholder only when the
base URL *is* the placeholder, exactly as the review prescribed:

- Changed: `src/cli/init_flow.rs:694` — `let placeholder_static = base.as_deref() == Some(PLACEHOLDER_BASE_URL);`
  (was `auth.is_none() || (auth.as_deref() == Some("static") && base.as_deref() == Some(PLACEHOLDER_BASE_URL))`).
  The now-unused `auth` binding was deleted; doc comment updated (`init_flow.rs:686-690`).
- Behavior: a `companion.base_url` set with `config set` (which never writes an `auth`
  key) survives a shared-posture re-run with `auth` still resolved as `static`; the
  placeholder-with-absent-auth default still derives `auth = google`.
- Evidence: new test `rerun_keeps_a_real_static_endpoint` (`init_flow.rs:1386-1405`) seeds
  a fully configured shared setup with a real `base_url` and **no** `auth` key, re-runs
  `init`, and asserts `companion.auth == Static`, `base_url == "https://my-llm.example/v1"`,
  `model == "gemini-3.7-flash"`, and `!written.contains("auth = \"google\"")` — passes.
  The existing `shared_posture_writes_a_working_config` still asserts the placeholder run
  derives `auth = google`, so the flip-on-placeholder path is regression-covered.

### P2-2 — The "offer to differ" for the cloud project

`shared_google_questions` now asks once, after the shared project answer, whether
inference runs in the same project (default yes); a no answers `companion.google_project`
separately. The offer fires only on the shared posture (Postgres/Cockroach store — the
local posture's companion is a static endpoint) and only when the companion side was
actually being filled (a previously differed `companion.google_project` is never
clobbered on a re-run).

- Changed: `src/cli/init_flow.rs:557-620` (`shared_google_questions`: split
  `companion_project_missing` at `558-563`; the embedder project is written to the file
  and the vault, then the offer at `579-613` writes `companion.google_project` — same
  project or the differed one, defaulting to the embedder project on an empty answer).
- New strings: `src/text/en.toml:188` `init.inference_same_project`,
  `src/text/en.toml:189` `init.inference_differ_project` (both via `text::get`).
- Evidence: new test `differ_offer_writes_a_separate_companion_project`
  (`init_flow.rs:1408-1421`) scripts `differ=no` + `inference-proj` and asserts
  `embedder.gemini_project == "proj"`, `companion.google_project == "inference-proj"`,
  `auth == Google`, and the vault still holds the embedder's project (`gemini-project`),
  which is the name the MCP env map references. All six existing shared-posture scripted
  tests gained one empty line after the project answer (differ default = same project)
  and still pass unchanged in their assertions.

### P2-3 — `config show`'s missing report omits the placeholder companion

`missing_config` now flags a static companion that still points at the shipped default.

- Changed: `src/config/show.rs:12-13` (`PLACEHOLDER_BASE_URL`, `LOCAL_MODEL` consts,
  mirroring `init_flow`'s), `src/config/show.rs:271-277` (push
  `config.missing_companion_endpoint` when `auth == Static` and
  `base_url == PLACEHOLDER_BASE_URL || model == LOCAL_MODEL`).
- New string: `src/text/en.toml:60` `config.missing_companion_endpoint`.
- Evidence: new tests `missing_config_flags_the_placeholder_companion` and
  `missing_config_does_not_flag_a_real_static_endpoint` (`src/config/show.rs:399-419`);
  the shipped default (`Config::default()`) reports the bullet, a real endpoint does not.
  The pre-existing `missing_config_reports_the_local_posture_sqlite_path` test still
  passes — its `!joined.contains("gemini")` assertion is unaffected by the new bullet's
  text.

### P2-4 — MCP news/artifacts offered and "wired" when the vault lacks the gemini secrets

`mcp_step` now gates the news/artifacts offer on the vault and re-stores the values from
config when missing, mirroring the coder key path (the remediation's first, safer option).

- Changed: `src/cli/init_flow.rs:824-856` — the config-based `has_google` is replaced by:
  1. re-store `GEMINI_PROJECT_SECRET` / `GEMINI_CREDENTIALS_SECRET` from
     `embedder.gemini_project` / `embedder.gemini_credentials` when the vault lacks them
     (values cloned out of the config first, since the vault write borrows `self`
     mutably), and
  2. `has_google = vault().get("gemini-project").is_ok() && vault().get("gemini-credentials").is_ok()`
     (`init_flow.rs:854-855`).
- Behavior: a setup configured via `mooshik secret set` + `config set` (or a recreated
  vault) now gets the offer with the env-map names actually resolvable at spawn; a
  config that genuinely has no google values still declines with `init.mcp_no_google`.
- Evidence: new test `mcp_offer_gates_on_the_vault_and_restores_missing_gemini_secrets`
  (`init_flow.rs:1424-1445`) seeds a config-complete-but-vault-empty setup, runs with a
  fake venv, and asserts `[mcp_servers.news]`/`[mcp_servers.artifacts]` are wired **and**
  `read_vault("gemini-project") == "proj"` and `read_vault("gemini-credentials") ==
  "/key.json"` — the re-store that pre-fix never happened. The existing
  `mcp_servers_are_wired_when_the_venv_is_there` test (first-run path, vault populated by
  `shared_google_questions`) still passes.

### P2-5 — Terminal echo is not restored when a secret read is interrupted

`read_no_echo` now restores termios from a RAII guard and installs temporary
SIGINT/SIGTSTP handlers that restore the original attributes before re-raising with the
default disposition.

- Changed: `src/cli/init_flow.rs:999-1094` —
  - `NoEchoRestore` guard (`init_flow.rs:1073-1094`): its `Drop` clears `ECHO_TERMIOS`,
    restores the original termios, and puts back the previous SIGINT/SIGTSTP
    dispositions. Every return path drops it.
  - `install_echo_handler` (`init_flow.rs:1036-1051`): installs `restore_echo_and_raise`
    for one signal with `SA_RESETHAND | SA_NODEFER`, returning the previous disposition;
    the two installs run inside `read_no_echo` (`init_flow.rs:1025-1026`).
  - `restore_echo_and_raise` (`init_flow.rs:1058-1068`): async-signal-safe handler —
    `tcsetattr` to restore echo, `signal(SIG_DFL)`, `raise` so Ctrl-C/Ctrl-Z takes its
    default effect (terminate / stop) with the terminal already restored.
  - `no_echo = false` (test) path and the `#[cfg(not(unix))]` stub are unchanged.
- Evidence: lib builds clean; the unix-only code follows the repo's established
  `sigaction` pattern in `src/tui/mod.rs` (`leave_on_signals`/`restore_signals`). The
  behavior (echo restored after an interrupted secret prompt) is not exercisable in a
  unit test (needs a real tty + signal); noted as the one finding verified by code
  inspection and build only.

### P2-6 — Local re-run silently defaults a chosen gemini embedder to bge_m3

On the local branch, when `embedder.kind == Gemini` with missing pieces and the file
already carries a gemini project or credentials key (progress past the kind question),
the kind question is skipped and `shared_google_questions` fills only the gaps —
mirroring the shared branch.

- Changed: `src/cli/init_flow.rs:487-497` in `embedder_step` — before
  `ask_embedder_kind_local`, `file_has(source, "embedder.gemini_project") ||
  file_has(source, "embedder.gemini_credentials")` short-circuits to
  `shared_google_questions()`. A fresh first run (neither key in the file — the shipped
  default writes `kind` but no project/credentials) still gets the kind question, so the
  local posture's bge_m3 default choice is preserved.
- Evidence: new test `local_rerun_keeps_a_chosen_gemini_embedder`
  (`init_flow.rs:1448-1476`) seeds `kind = "gemini"` + `gemini_project = "proj"` (no
  credentials), re-runs with only a credentials answer, and asserts `kind == Gemini`
  (kept), `gemini_project == "proj"` (preserved), both credential keys and the vault
  filled with the new path, and `!output.contains("bge_m3")` (kind question never asked).
  `local_posture_writes_sqlite_and_a_local_companion` (fresh run, kind question asked,
  bge_m3 default taken) still passes.

### P3-7 — Retrying a failed Google inference check re-runs without re-asking

`verify_inference`'s retry now re-asks the likely wrong answer for Google auth too:
the credentials path, mirroring the store (DSN) and embedder (credentials) retries.

- Changed: `src/cli/init_flow.rs:791-800` — the `else` branch (non-Static auth) re-asks
  `init.embedder_gemini_credentials` with echo off, writes it to both
  `embedder.gemini_credentials` and `companion.google_credentials`, and stores it in the
  vault (`GEMINI_CREDENTIALS_SECRET`) — the same shape the embedder retry already used.
- Evidence: new test `google_inference_retry_re_asks_the_credentials`
  (`init_flow.rs:1478-1497`) with `fail_inference = true`: the scripted retry answers
  `y` then a new path; the test asserts both credential keys and the vault hold the
  re-asked value, and the run closes with `Unverified` + the inference item.

### P3-8 — Unused `config` binding in the `read_vault` test helper

- Changed: `src/cli/init_flow.rs:1213` — the `let config = Config::load_at(&root)`
  binding is deleted (the `root` handle is still used by `Vault::open_at`).
- Evidence: `cargo build --tests` emits no `unused variable` warning (see below).

---

## Verification (exact commands and results)

All commands ran from `/home/nryn/work/mooshik` with the environment scrubbed of
`MOOSHIK_*` and `LAMBO_*` (ambient `LAMBO_POSTGRES_DSN` is what makes
`cli::tests::moving_the_store_is_refused` fail without scrubbing):

```
env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo build
# -> Finished dev profile; zero warnings

env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo build --tests
# -> Finished dev profile; zero warnings (P3-8 acceptance)

env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo test --lib cli::init_flow
# -> 12 passed; 0 failed  (7 pre-existing, all updated for the differ offer, + 5 new)

env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo test --lib config::
# -> 73 passed; 0 failed  (71 pre-existing + 2 new in show.rs)

env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo test --lib text::
# -> 4 passed; 0 failed

env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo test --lib memory::resolve
# -> 7 passed; 0 failed

env -u MOOSHIK_VAULT_PASSPHRASE -u MOOSHIK_POSTGRES_DSN -u MOOSHIK_HOME \
    -u MOOSHIK_COMPANION_AUTH -u LAMBO_POSTGRES_DSN -u DATABASE_URL \
    cargo test --lib cli::tests
# -> 36 passed; 0 failed  (the ambient-LAMBO_POSTGRES_DSN failure is gone with
#    the scrubbed env; tests.rs untouched)
```

Line cap (CI `ci.yml:55-57` semantics, `wc -l` over `git ls-files '*.rs'`):

```
over=$(wc -l $(git ls-files '*.rs') | awk '$NF != "total" && $1 > 1500')
# -> empty: cap OK. src/cli/init_flow.rs is 1493 lines; src/config/show.rs is 422.
```

## Test counts (before → after)

| suite | before | after |
|---|---|---|
| `cli::init_flow` | 7 | **12** (5 new: P2-1, P2-2, P2-4, P2-6, P3-7) |
| `config::` (incl. `show`) | 71 | **73** (2 new: P2-3) |
| `text::` | 4 | 4 |
| `memory::resolve` | 7 | 7 |
| `cli::tests` | 35 pass + 1 ambient-env failure | **36** (all pass, env scrubbed) |

## Left unfixed

Nothing. All 8 findings remediated. Caveats, for the record:

- P2-5 (termios/signal safety) is verified by build + code inspection only; the
  interrupted-read behavior needs a real tty and is not unit-testable in this suite
  (`no_echo = false` in tests by design).
- `init_flow.rs` sits at 1493/1500 lines — under the cap, with ~7 lines of headroom.
