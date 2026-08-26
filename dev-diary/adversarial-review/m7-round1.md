# M7 adversarial review — round 1

Branch `m7-cli-sweep`, HEAD `4dbf798`. Reviewer drove the real binary
(`cargo build --locked`, dev profile) with its own homes under `/tmp`, plus
source-level classification and four mutations. Tree was left clean; all
mutations reverted (`git status` clean before and after every mutation). No
non-deterministic file state observed during review (an early apparent miss of
`m7-implementation.md` was the reviewer's path error — the file lives under
`dev-diary/adversarial-review/`).

## Verification performed

* Independently classified every variant of `ConfigError` (12), `HomeError`
  (5), `VaultError` (14), `MemoryError` (2), `CompanionError` (9) from source
  and compared against `cli::is_user_error`. Mapping matches the implementer's
  table exactly — see finding P2-d for the residual class/content mismatches.
* Behavioral probes against a real passphrase-mode vault and the real
  Postgres/Gemini stack (`LAMBO_POSTGRES_DSN` ambient): empty value set,
  flag-like name, format-brace value, wrong passphrase, missing home, bare
  usage forms, recall/stats under a held session lease.
* Ran three documented examples for real: `mooshik init`, `mooshik recall
  "deploy checklist"` (returned live hits from the shared store), `mooshik
  stats` (full health block) — all parse and behave as written.

## Findings

### P1-a — Empty secret value exits 1, not 2 (`src/cli.rs:455-502`)

The classifier maps `VaultError::MissingValue` to User/exit 2, but the CLI's
own input normalization raises **untyped** errors: both
`normalize_environment_value` and `normalize_stdin_bytes` return
`anyhow!(text::get("vault.missing_value"))` with no `VaultError` anywhere in
the chain. `is_user_error` walks the chain, finds no known class, and fails
internal. Live-verified both paths:

```
$ MOOSHIK_SECRET_VALUE="" mooshik secret set gamma; echo $?
Secret values must be non-empty. Provide MOOSHIK_SECRET_VALUE or stdin.
1                                    ← must be 2
$ printf '' | mooshik secret set gamma; echo $?
1                                    ← must be 2
```

Message text is right; the exit code — this milestone's headline deliverable —
is wrong for exactly the operator mistake the classifier table says it covers.
Fix: construct `VaultError::MissingValue` (typed) at these call sites instead
of bare `anyhow!`. Same latent issue in the stdin-read and invalid-UTF-8 arms
(`vault.io_failed` untyped) — those classify 1 defensibly, but they should be
typed too so the mapping stays the single decision point.

### P2-b — A cause-chain print inside `Failure::report` survives every pin

`report()` is declared "THE one place an error reaches the terminal", but the
pins cover only `rendered()` (data level) and the literal `{err:#}` in
main.rs (source level). Mutation M2 below re-introduced
`eprintln!("{:#}", …)` (anyhow alternate = full cause chain) directly inside
`report()`: **all 187 tests still pass**, including
`report_renders_the_top_level_message_never_the_wrapped_chain`. The wrapped-
DSN guarantee is enforced one layer away from the actual print. Fix: pin
`report()`'s observable output (e.g. assert on captured stderr via a seam, or
a source pin asserting `report`'s body formats `self.rendered()` with `{}`).

### P2-c — Backend failures flatten actionable detail; lease conflicts mislead

`MemoryError::Backend(_)` renders the fixed `memory.backend_failed`
("Check the store, embedder, and credentials") regardless of cause. When a
chat/serve holds — or recently held, until the lease TTL lapses — the
single-writer lease for the same session id, one-shot `recall`/`stats` (which
open their own handle on the identical session) get lambo's
`LamboError::Conflict("session … already held by another writer … force a
takeover …")`. That operator-actionable message never surfaces; the printed
remediation is factually wrong advice. Live: with `serve` up/recently killed,
`recall` printed the generic message and exited 1 (reproduced twice);
standalone recall 3/3 exit 0. Exit class 1 for Conflict is defensible; the
message is not. Fix options: map known-safe `LamboError::Conflict` to its own
en.toml key naming the other writer (its payload contains no secrets), or say
"another Mooshik process may be using workspace memory" instead of pointing at
store/embedder credentials. Related honesty note: main.rs scopes destructor
preservation to the success path, so a classified-failure exit skips chat's
executor `Drop` — lease held until TTL and write-behind tail lost like a
crash. Defensible, but the diary should say so.

### P2-d — Variants whose en.toml messages give operator fixes classify exit 1

The help afterword defines 2 = "the request was refused … configuration, or a
name that does not exist" and 1 = "retrying or reporting is the next step, not
reconfiguring". These variants print reconfiguration-style instructions yet
exit 1: `HomeError::{UnsafePath, MigrationRequired, LayoutConflict}`
("Back it up, move it aside, then run `mooshik init`"; "Move the old vault
directory"), `VaultError::{InvalidFormat, UnsafePath, LockFailed, Keyring}`
("Restore a valid vault…"; "Select passphrase mode and provide
MOOSHIK_VAULT_PASSPHRASE"), `CompanionError::{InvalidResponse, ToolLoop}`
("Check the endpoint…"). Each individually arguable, but the class contradicts
the stated convention and the afterword text scripts will read. Either widen
the User set for these or reword their messages; pick per-variant deliberately.

### P2-e — Chat's tool boundary still prints raw error detail, against the sweep's own rationale

The sweep routed `Drop`-close and gate-panic notices into en.toml *explicitly
because* "the raw LamboError display is outside the vault-value guarantee" —
yet `tools/mod.rs:409` (`lambo_err`) still does `eprintln!("{what}: memory
error: {error}")` with the raw `LamboError` Display, and `tools/mod.rs:433`
prints the raw panic payload (`tool {name} panicked:
{panic_message(payload)}`). A `LamboError::Store` wrap of a connection failure
can carry host/user DSN material straight to stderr, and the implementation
diary's claim "the terminal sees exactly the top-level message, never a chain"
is false for these paths. Pre-existing lines, but M7 was the sweep with
authority over exactly this. Apply the same treatment as `gate_panicked`
(fixed en.toml notice, detail dropped).

### P3-f — Parser pin tokenizes raw TOML, not rendered help

`documented_mooshik_examples` reads `include_str!("text/en.toml")` raw. The
recall example is stored as `` `mooshik recall \"deploy checklist\"` ``; the
tokenizer keeps backslashes, so clap receives query `\deploy checklist\` — the
pin passes on a mangled token stream, not the string users see
(`mooshik recall "deploy checklist"`, which also parses). Today harmless;
tomorrow an escaping change could diverge silently. Unescape TOML basic strings
before tokenizing (or extract from `text::get` values).

### P3-g — `tools.close_failed` states what failed but not what to do

Floor item 1 demands what/why/next. "Workspace memory did not shut down
cleanly." stops there; siblings always add a next step.

### P3-h — Leading-hyphen secret names accepted

`validate_name` allows `-flaglike`; `secret list` then emits strings that break
naive `| xargs mooshik secret get` consumers. Reject leading `-`.

### P3-i — Minor voice drift in `[tools]`

Model-facing strings mix styles: "A recall knob is out of its allowed range"
(capitalized sentence) beside "query must be a non-empty string" (lowercase
spec style). Consistent within audience would be ideal; lowest priority.

### P3-j — Diary omission (accuracy, not error)

Everything else in the live log checked out — see plausibility section.

## Mutation table

| # | Mutation | Pin expected to catch | Result |
| --- | --- | --- | --- |
| M1 | Remove `VaultError::NotFound` from the User set in `is_user_error` | `exit_codes_distinguish_user_error_from_internal_failure` | **CAUGHT** (test failed) |
| M2 | Cause-chain print (`{:#}`) inside `Failure::report` itself | `report_renders_…never_the_wrapped_chain` | **SURVIVED** — 187 pass; no test observes `report`'s print (finding P2-b) |
| M3 | `Failure::rendered` appends `root_cause()` (wrapped-chain leak at data level) | `report_renders…`, `backend_failures…`, `a_vault_value…` | **CAUGHT** (3 failed) |
| M4 | Corrupt en.toml example `mooshik stats` → `mooshik stats --verbose` | `every_documented_example_parses_as_written` | **CAUGHT** (test failed) |

## Behavioral verification log (reviewer-run)

| probe | result |
| --- | --- |
| `init` on fresh home | 0, "Mooshik home initialized." |
| `stats` / `recall` on fresh home vs shared store | 0; recall returned the live `m7 cli sweep marker` concept — independent corroboration of cross-command memory through the real stack |
| `secret get nosuchname` | exit 2, `vault.not_found` verbatim, stored value absent |
| wrong passphrase `secret get` | exit 2, auth message, probed value absent from output (grep 0 hits) |
| value `{value} {query} %s $x` set+get | round-trips verbatim; no format-injection in any `.replace`/`println` path |
| empty value (env and stdin) | correct message, **exit 1** (P1-a) |
| missing home `recall` / `stats` | exit 2, message names `mooshik init` |
| bare `recall`, unknown subcommand, bare `mooshik`/`secret`/`config` | clap usage/help, exit 2 uniformly |
| `--help` | exit 0; afterword carries the exit-code convention |
| `recall` with serve holding/expiring lease | exit 1, generic backend message (P2-c) |
| `config show` | `dsn = "***REDACTED***"`; no api_key line when unset |

## Live-log plausibility

Commit times 09:32–09:39 (+0530) precede the reviewer's probes (~09:57). The
diary's step-7 marker concept exists in the shared store exactly as recorded,
with the documented render shape (`content · entity · relevance N.NN`);
step-9 stats fields match `render_stats`' output one-for-one; gate counts
below match the diary verbatim. One reviewer probe saw `stats` succeed while
`recall` had failed seconds earlier against the same lease window — consistent
with TTL-lapse timing, folded into P2-c rather than treated as an
inconsistency.

## Gates (run once, at end, tree clean)

```
cargo fmt --all -- --check                              clean
cargo clippy --all-targets --locked -- -D warnings      clean
cargo test --locked                                     187 passed · 0 failed · 1 ignored
```

## Verdict

**REJECT (one round of remediation; scope is small).**

The architecture is right — one classifier, one rendering path, honest
redaction-scope documentation, and the live verification is genuinely live
(this reviewer reproduced its central claim independently). But the milestone's
headline contract ships with a counterexample (P1-a: a canonical user error
exiting 1), and the two pins that make the leakage/classification claims
durable both have holes proven by surviving mutations (M2) and vacuous passes
(P3-f). Required for re-review:

1. P1-a — type the input-normalization errors as `VaultError` variants.
2. P2-b — a pin that fails when `Failure::report` grows any non-`{}` format.
3. P2-c — truthful remediation for backend/lease-conflict failures.
4. P2-d/P2-e — pick a side per variant (message or class) and finish the
   stderr sweep in `tools/mod.rs`.
