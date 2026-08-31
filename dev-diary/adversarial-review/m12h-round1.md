# M12h — the guided first run: adversarial review, round 1

**REMEDIATE: 6 P2, 2 P3 at HEAD d702609 (+ 0b96d3b docs record)**

Reviewer: `M12hReview1` (adversarial pass of the implement → review → remediate cycle).
Spec: `dev-diary/PLAN.md` "### M12h — what a first run has to say" (lines 1064–1297) plus the
M12h bullet (746–756). The implementation record (`m12h-implementation.md`) was checked against
the code, not trusted: its test counts, the byte-identical non-TTY claim, and the "0 warnings"
claim were all re-verified. Evidence below was produced with read-only checks: `git show`,
targeted `cargo test` runs, and a live `init --non-interactive` + `config show` on a scrubbed
home. No formatters, linters, or the full suite were run; no files were modified.

---

## Findings

### P2 — 1. Don't clobber a real static endpoint when `companion.auth` is absent

`src/cli/init_flow.rs:657-672` (`derive_shared_inference`):

```rust
let placeholder_static =
    auth.is_none() || (auth.as_deref() == Some("static") && base.as_deref() == Some(PLACEHOLDER_BASE_URL));
if placeholder_static {
    self.set("companion.auth", "google")?;
}
```

The shipped `DEFAULT_TOML` never writes an `auth` key (the resolved default is `static`). So a
user who configured a **real** local endpoint with `mooshik config set companion.base_url
https://my-llm.example/v1` (a settable, deliberate action) still has `auth` absent in the file.
Re-running `init` on the Postgres branch then treats `auth.is_none()` as "still the shipped
placeholder" and rewrites `companion.auth = "google"` — silently abandoning the working endpoint
(the `base_url` they set is left in the file but ignored by Google auth) and dragging them into
the Google project/credentials questions. The guard already distinguishes "real static" from
"placeholder static" via `base_url`; the `auth.is_none()` disjunct breaks exactly that
distinction. Impact: a re-run converts a deliberately configured static inference setup to
Vertex without ever asking — a violation of the plan's "confirms rather than clobbers anything
already configured".

Remediation: treat as placeholder only when the base URL is the placeholder:

```rust
let placeholder_static = base.as_deref() == Some(PLACEHOLDER_BASE_URL);
```

(auth then only ever flips when the file still carries the shipped default URL; `auth = google`
with a placeholder base is a no-op set).

### P2 — 2. The "offer to differ" for the cloud project is not implemented

Plan (line 1107–1110): "The cloud project. Asked **once**. It fills both
`embedder.gemini_project` and `companion.google_project`, **with an offer to differ**, because a
cross-project setup is real: the deployed ingester runs its service account from `mooshik` with
`roles/aiplatform.user` on `nryn-personal`."

`src/cli/init_flow.rs:561-568` (`shared_google_questions`) asks one question and writes the same
answer to both keys; the only nod is the prompt text "write one; differ later if you must"
(`init.embedder_gemini_project` in en.toml), which tells the user to hand-edit afterwards. There
is no in-flow offer to name a different `companion.google_project`. A cross-project setup — the
plan's stated reason for the offer — cannot be expressed through `init`; the user must discover
the asymmetry later as a 404/403. Plan point half-met.

Remediation: after the shared project answer, ask once whether inference runs in the same
project (default yes); if no, ask `companion.google_project` separately before deriving
inference.

### P2 — 3. `config show`'s missing report omits the placeholder companion

`src/config/show.rs:247-265` (`missing_config`) emits companion bullets only when
`companion.auth == Google`. On the shipped default (auth `static`, `base_url =
http://127.0.0.1:8080/v1`, `model = local-model` — the exact "local posture nobody has running"
state the milestone exists to end) the report says nothing about the companion. Verified live:
fresh scrubbed home → `init --non-interactive` → `config show` prints the store-DSN and the two
gemini bullets, but nothing flags that inference still points at a dead localhost endpoint with
`local-model`. The interactive flow itself treats that companion as unset
(`companion_needs_asking` → placeholder → true, init_flow.rs:675-695), so `init` and
`config show` disagree about the same file. The plan's done-when for the scripted route is
"`mooshik config show` tells anyone who took that route what is still missing"; a user who
follows the printed bullets ends with a config whose chat still cannot work.

Remediation: in `missing_config`, when `companion.auth == Static` and
`base_url == PLACEHOLDER_BASE_URL` (or `model == "local-model"`), push a
`config.missing_companion_endpoint` bullet ("companion.base_url/model still point at the shipped
local default").

### P2 — 4. MCP news/artifacts offered and "wired" when the vault does not hold the gemini secrets

`src/cli/init_flow.rs:781-833`: the `has_google` gate reads the **config** (`embedder.gemini_project`
+ `gemini_credentials` non-empty), not the vault. The env map written by `wire_server` references
vault secret **names** `gemini-project`/`gemini-credentials`, which only `shared_google_questions`
ever stores (lines 565-567, 585-587) — and on a re-run it skips storing them when the config is
already complete. So for any setup configured outside `init` (the documented `mooshik secret set`
+ `config set` path, or a recreated vault), `init` offers news/artifacts (default yes), writes
`[mcp_servers.news]` with `env = { MOOSHIK_GEMINI_PROJECT = "gemini-project", ... }`, says
"news wired" — and the server then never spawns: `mcp_host` resolves the env at spawn, fails
closed on the missing secret (`mcp_secret_missing`), and only reports it in chat diagnostics.
The coder offer already guards the same class of problem (`vault().get(secret_name).is_err()`,
line 841); the news/artifacts path has no equivalent.

Remediation: gate the news/artifacts offer on `vault().get("gemini-project").is_ok() &&
vault().get("gemini-credentials").is_ok()` (re-storing the values from config when they are
missing, like the coder key path), or state in `mcp_wired` that the env names must exist in the
vault.

### P2 — 5. Terminal echo is not restored when a secret read is interrupted

`src/cli/init_flow.rs:943-961` (`read_no_echo`) clears `ECHO` on `STDIN_FILENO`, reads, then
restores. The restore runs only on the normal return path — there is no `Drop` guard and no
signal handling. SIGINT (Ctrl-C) or SIGTSTP (Ctrl-Z) during a secret prompt kills/stops the
process with default disposition, the final `tcsetattr` never executes, and the user's terminal
is left with echo disabled (appears "typed nothing"; fixed only by `stty echo`). This is a
first-run surface — exactly where a user is most likely to abort a secret prompt.

Remediation: restore termios from a RAII guard and, during the no-echo read, install a temporary
SIGINT/SIGTSTP handler that restores the original attributes before re-raising; keep
`no_echo`/test path unchanged.

### P2 — 6. Re-run on the local path silently defaults a chosen gemini embedder to bge_m3

`src/cli/init_flow.rs:484-528`: on the sqlite branch, `embedder_needs_asking` returns true when
`kind == Gemini` and project or credentials are missing, and then `ask_embedder_kind_local`
re-asks the **kind** question — whose default is bge_m3 (`"" | "2"` → `set("embedder.kind",
"bge_m3")`). An interrupted first run (gemini chosen, project entered, credentials never
completed — or a failed verification the user walked away from) leaves `kind = "gemini"` in the
file; the re-run then re-asks "1) gemini 2) bge_m3 [2]" and a plain Enter silently replaces the
user's earlier deliberate gemini choice with bge_m3. This violates the plan's re-run contract —
"asks only for what is still unset, and confirms rather than clobbers anything already
configured" — and the shared branch does it right (kind never re-asked; only the missing
project/credentials are filled by `shared_google_questions`).

Remediation: when `embedder.kind == Gemini` on the local path, skip `ask_embedder_kind_local`
and go straight to `shared_google_questions()` (ask only for the missing pieces), mirroring the
shared branch.

### P3 — 7. Retrying a failed Google inference check re-runs without re-asking

`src/cli/init_flow.rs:745-758` (`verify_inference`): on retry, only the `Static` branch re-asks
(the base URL). For `auth = google` (the shared posture), choosing "Retry? [Y/n]" loops straight
back into the identical verification with an unchanged config — the retry is a no-op until the
user gives up and types `n`. The plan requires "offer a retry **and allow continuing**"; a
retry that cannot change the outcome is a trap. (Store and embedder retries re-ask the likely
wrong answer; this one should too — e.g. re-ask the credentials path — or state that nothing
new can be tried.)

### P3 — 8. Unused `config` binding in the `read_vault` test helper

`src/cli/init_flow.rs:1082`: `let config = Config::load_at(&root).unwrap();` is never used.
`cargo build --tests` warns `unused variable: config` — contradicting the implementation
record's "0 warnings" claim (the plain lib build is clean; CI's `cargo clippy -- -D warnings`
does not cover test targets by default, so this is cosmetic, but it is patch-introduced noise).
Drop the binding.

---

## What was checked and passed

- **Non-TTY / `--non-interactive` path is byte-identical.** `initialize_unattended`
  (`src/cli/memory_cmd.rs:24-35`) is the old `initialize` body verbatim (`git show
  d702609^:src/cli/memory_cmd.rs` diffed clean). The only output deviation anywhere is the
  intended one: `memory.missing_dsn` now leads with the durable fix.
- **Plan point coverage.** Opening sentences, vault statement (keyring vs passphrase, why),
  posture first with shared default, store (Postgres-you-run / cloud / sqlite, Auth-Proxy
  caveat on its own line), embedder with the sticky warning at the moment of choosing, project +
  credentials asked once each and written to both sides, derived shared inference
  (`auth = google`, `google_location = global`, `model = gemini-3.7-flash`) with the
  two-locations trap stated out loud, local inference (URL, model, optional bearer key),
  verify-each-answer with retry-or-continue and a closing unverified list, MCP offer gated on
  the venv with one-keystroke decline, `mooshik tui` + pane-starts-empty + positional-ambient
  advice (walk figures not quoted, per the plan's re-measure instruction). `memory` never
  offered; `fixture` never offered; permissions never raised; no extra questions beyond the
  plan's list, in the plan's order.
- **The three non-interactive fixes.** (1) `config show` missing report live-verified (store
  DSN durable fix, gemini project/credentials bullets) — completeness caveat is P2 #3. (2)
  `memory.missing_dsn` leads with `mooshik secret set` + `config set store.dsn_secret` and
  offers the env escape hatch second; live-verified via `init --non-interactive` on a scrubbed
  home (exit 2, correct message). (3) `gemini-2.5-flash` gone from `set_after_help` and the
  `DEFAULT_TOML` comment; remaining occurrences are test fixtures and historical dev-diary
  files only.
- **SETTABLE additions land and validate.** `store.path` and `embedder.gemini_credentials`
  (Kind::Path) present; `every_settable_key_is_reachable_and_actually_lands` covers them;
  `store.dsn` / `companion.api_key` still refused by name.
- **String discipline.** All flow strings via `text::get`; `[init]` section keys cross-checked
  against every `text::get("init.*")` call site — no missing keys, no Rust literals in
  user-facing positions (constants like `PLACEHOLDER_BASE_URL`, `SHARED_MODEL`, vault names are
  data, not copy).
- **Secrets.** DSN, credentials path, coder key, bearer key all read with echo off, stored only
  in the vault; config.toml holds names only; scripted tests assert the secret is absent from
  file and output and present in the vault; writes go through `apply_setting` +
  `write_private_at` (0600, atomic, no symlink).
- **Tests (targeted, env-scrubbed):** `cli::init_flow` 7/7, `cli::tests` 35/35 (36 total — the
  one failure, `moving_the_store_is_refused...`, is the documented ambient-`LAMBO_POSTGRES_DSN`
  conflict: `tests.rs` is untouched by the patch, and the test passes with the env scrubbed),
  `config::` 71/71, `memory::resolve` 7/7, `text::` 4/4. Scripted tests assert on config.toml,
  vault contents, and transcript (not just exit codes).
- **Line cap.** CI enforces 1500 lines per `.rs` (`.github/workflows/ci.yml:55-57`); all
  touched files fit — `init_flow.rs` is 1246. The plan's "1000-line" figure is stale relative to
  the enforced cap; the implementation record documents this.
- **Cross-boundary dispatch.** `mcp_step` → `append_mcp_block` → `mcp_host` env resolution
  traced (fail-closed at spawn — the basis for P2 #4); coder wiring reuses
  `configure::coder_agent_secret` / `apply_coder_config` / `find_coder_command` with the
  pre-existing coder tests passing unchanged.
