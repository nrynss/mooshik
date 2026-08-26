# M7 implementation record — CLI validate-and-repair sweep

Branch `m7-cli-sweep` (cut from `main` at `876f849`). Three commits:

1. `feat(cli): add recall and stats commands with classified exit codes`
2. `feat(memory): one-shot recall and stats operations`
3. `refactor(tools): route remaining stderr notices through en.toml`

## New surface

### `mooshik recall <query>`

One-shot search over workspace memory via `crate::memory::recall(config, query)`
→ `Memory::recall(RecallQuery)`. Defaults pinned in `memory::ops`:
`top_k 5`, `max_tokens 200`, `traversal_depth 0`. Opens its own `Memory`
handle and closes it cleanly; renders human output:

```
Matches for 'live m7 cli sweep marker':

  1. live m7 cli sweep marker
     entity · relevance 1.63
```

Empty result names the query and the next step (`mooshik chat`). Warnings
render under a `Warnings:` header when lambo reports any.

**Scope honesty (documented in code, both in `memory::ops::recall` and
`cli::render_recall`):** recall prints to the LOCAL operator. It is
deliberately *not* routed through chat's egress redaction — nothing recalled
here reaches a model or history, so a vault value that happens to match
concept text stays visible to the person who owns the machine.

### `mooshik stats`

Session health via `crate::memory::stats(config)` → `Memory::stats()`,
rendered as labeled lines: concept counts (total/canonical/embedded), node
and edge counts, write-behind log depth, flush lag, dead-lettered batches,
durability degraded flag, background cycle counts with canonization failures.
Same config/env resolution as every other command (`Config::load_at`, so
`MOOSHIK_*` overlays apply identically to chat).

## Decisions

### Exit codes

`0` success · `2` user error · `1` internal failure, decided once in
`cli::Failure` (`User`/`Internal` over `anyhow::Error`) and applied through
one classification function that walks the cause chain downcasting to the five
known error enums:

- **User (2):** every `ConfigError`; `HomeError::MissingHome`;
  `VaultError::{NotFound, InvalidName, MissingValue, NulByte, InputTooLarge,
  MissingPassphrase, Authentication}`; `MemoryError::MissingDsn`;
  `CompanionError::{Unreachable, Timeout, HttpStatus, TurnTooLarge}`.
- **Internal (1):** everything else — unknown classes fail visible rather than
  punish a script with a misleading "you did it wrong".

Clap answers usage errors itself with exit code 2 (verified live below), so
the convention holds end to end. Documented in `--help`'s afterword:
"Exit codes: 0 success · 2 the request was refused (bad usage, configuration,
or a name that does not exist) · 1 unexpected internal failure."

### One error-rendering path

`Failure::report` in cli.rs is THE place an error reaches the terminal; the
old `eprintln!("{err:#}")` chain print in main.rs is gone (source-pinned by
test). Rationale: every error type already renders what/why/next through
`text/en.toml` in its top-level `Display`, but wrapped sources carry no such
guarantee — `MemoryError::Backend(LamboError)` can wrap a store error whose
detail names DSN material. The terminal therefore sees exactly the top-level
message, never a chain. The pin test constructs a backend failure whose chain
provably contains fake connection material and asserts the rendered message
does not.

No `{:?}` formatting of errors exists anywhere in non-test source (audited);
no `.context()`/`anyhow!` call site embeds dynamic values other than fixed
en.toml strings.

### SecretToken-in-errors verification (floor item 3)

- Behavioral pin: store `"s3cret-alpha-value-m7"` under name `alpha` in a real
  passphrase vault, request missing name `beta` → exit code 2, rendered message
  equals `vault.not_found` verbatim, and the stored value appears nowhere.
- Source-chain probe above proves even wrapped detail cannot print.
- Structural: `VaultError`'s `Display` maps each variant to one fixed en.toml
  key; no variant carries payload (audited, M4/M6 lift table unchanged).
- Live probe: with `m7probe = live-m7-secret-value-9f3` in the real vault,
  `secret get nosuchname` exits 2 printing only "That secret does not exist.
  Check the name and try again." — grep for `9f3` on the output: absent.

### Voice-consistency checklist (normalized this pass)

- clap builder API everywhere (no derive); all help strings from en.toml —
  pre-existing convention held; new commands follow it and are pinned by test.
- Command about-lines are imperative verb-first sentence case ("Search…",
  "Print…") matching init/config/chat/secret/permissions.
- Two inline stderr literals routed into en.toml: the memory-close notice in
  `tools::MemoryTools::Drop` ("memory close: …" → `tools.close_failed`, detail
  dropped because the raw LamboError display is outside the vault-value
  guarantee) and the permission-gate panic notice in `tools::GatedTools`
  ("permission gate panicked: …" → `permissions.gate_panicked`; the raw panic
  payload no longer forwards to the terminal at all — fail closed, refuse,
  say why).
- Concept kinds render as lowercase words from en.toml (`memory.kind_*`),
  never Rust variant names (pinned: rendered recall output must not contain
  "Entity").
- Placeholder convention `{name}`/`{tool}`/`{query}`/`{score}` matches the
  existing `.replace("{x}", …)` style.

### config show consistency

Verified live: `[store] dsn` prints as `***REDACTED***`, companion api_key
omits entirely when unset, `[embedder]` shows project/location/model/
credential PATH (not contents). `[permissions]` and `[tools.scratch.env]`
sections render names only when configured and are omitted otherwise —
consistent with how they load (M5/M6 tests already pin this; live run below
shows the unset case).

### Help examples runnable (floor item 6)

New pin `every_documented_example_parses_as_written`: extracts every backticked
span starting with `mooshik ` from en.toml, tokenizes (double-quote aware),
and asserts each parses against the real clap command tree. Current documented
examples: `mooshik init` (three places), `mooshik chat`,
`mooshik recall "deploy checklist"`, `mooshik stats`. The empty-recall message
was deliberately worded without a bare `` `mooshik recall` `` span so the
extraction stays sound (a bare example would fail its own required argument).

## Tests added

- `exit_codes_distinguish_user_error_from_internal_failure` — variant-by-
  variant mapping for both codes.
- `backend_failures_classify_internal_but_render_the_mapped_message`.
- `report_renders_the_top_level_message_never_the_wrapped_chain` — chain
  carries fake DSN material, rendering provably does not; main.rs must not
  contain `{err:#}`.
- `a_vault_value_never_reaches_a_rendered_error` — behavioral vault probe.
- `every_documented_example_parses_as_written` (+ tokenizer helpers).
- `recall_and_stats_help_come_from_text`; bare `mooshik recall` is a usage
  error, not a panic.
- `recall_render_names_hits_and_warns_without_leaking_types` /
  `stats_render_reports_health_in_one_voice` — pure render pins.
- `one_shot_recall_and_stats_run_against_fixture_memory` — same-handle recall
  finds freshly derived concepts; documents that an in-memory store lives and
  dies with its handle, so the one-shot wrappers answer from whatever the
  configured store holds (cross-command persistence needs the durable store —
  proven live below).

Default suite stays net/model-free: 187 passed, 0 failed, 1 ignored
(the operator-run live round-trip).

## Live verification

Machine home `~/.mooshik`: store.kind=postgres (Cloud SQL), embedder.kind=
gemini (Vertex, gemini-embedding-001, project nryn-personal / us-central1),
companion = Vertex OpenAI-compat endpoint, model google/gemini-3.7-flash.
Env sourced from worktree `.env`. Companion key minted fresh per turn with
`gcloud auth print-access-token` (`/tmp/gcp-token.sh` did not exist; same
mechanism, token never recorded anywhere). All outputs below redact DSN and
token material.

| step | command | exit | result |
| --- | --- | --- | --- |
| 1 | `init` (idempotent on existing home) | 0 | `Mooshik home initialized.` |
| 2 | `config show` | 0 | sections vault/session/store/embedder/daemon/companion; `dsn = "***REDACTED***"`; gemini project/location/model + credential path shown |
| 3 | `printf 'live-m7-secret-value-9f3' \| secret set m7probe` | 0 | piped stdin accepted (no TTY needed) |
| 4 | `secret list` | 0 | `m7probe` — NAME only |
| 5 | `secret get m7probe` | 0 | value returned (local print allowed) |
| 6 | `permissions` | 0 | resolved grants table, defaults marked |
| 7 | `chat` (derive marker via Vertex + Cloud SQL) | 0 | tool loop ran; model called lambo_derive; reply: `Stored concept `'live m7 cli sweep marker'` (entity) successfully.` (~29 s) |
| 8 | `recall "live m7 cli sweep marker"` | 0 | hit 1: `live m7 cli sweep marker · entity · relevance 1.63`; hit 2: older unrelated marker at 0.57 — **the concept written by chat in step 7 came back**, proving cross-command memory through the real stack (~21 s) |
| 9 | `stats` | 0 | concepts: 2 total, 0 canonical, 2 embedded · graph: 5 nodes, 4 edges · log depth 0 · flush lag 0.0s · dead letters 0 · degraded: no |

Error-path probes (same session):

- `secret get nosuchname` → exit **2**, message "That secret does not exist.
  Check the name and try again."; grep for the stored value `9f3`: absent.
- `recall` against a missing `MOOSHIK_HOME` → exit **2**, message naming
  `mooshik init` as the fix.
- unknown subcommand → clap usage text, exit **2**.
- `--help` → exit **0**, afterword carries the exit-code convention.

Fixes required during live runs: none — every step passed as implemented.

## Gates

```
cargo fmt --all -- --check                 clean
cargo clippy --all-targets --locked -- -D warnings   clean
cargo test --locked                        187 passed, 0 failed, 1 ignored (live)
```

Live steps 1–9 plus probes: all green against real Cloud SQL + Vertex Gemini.
