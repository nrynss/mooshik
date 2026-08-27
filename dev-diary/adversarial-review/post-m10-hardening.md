# Post-M10 hardening — session review, 2026-08-27

Work between M10 shipping and M11 starting. Not a milestone: a macOS
bring-up, two root causes behind M9's null result, and the changes that make
the product's own claims true. Recorded here because the next session starts
from it.

## What landed

| Commit | What |
| --- | --- |
| `ff734ea` | macOS bring-up — 35/212 tests were failing on Darwin |
| `763e17e` | Historical `event_time` carried into the graph |
| `9130afe` | Dockerfile pinned a stale lambo; pin the pin |
| `075cd82` | The durability gate never fired |
| `8072c3f` | A week of Neom's workspace, built to earn canonization |
| `8581368` | Compile the offline store and embedder |
| `9e4d5e4` | A news MCP server |
| `bf38629` | A failed MCP write was read as a success |
| `6d3298b` | `news_mcp` importable from any pytest rootdir |
| `72e44e9` | Recall wired into chat |
| *(this)* | Config write path, database-move guard, Google auth |

## The three that were silent

Each looked healthy and was not. Worth remembering as a class.

**Canonization promoted nothing** — and the cause was not the policy. Lambo's
solo score counts recurrence over `about_time`, but the MCP wire had no field
to carry it, so a decade of history landed stamped with the flush clock,
every concept scored ~1 session against a Candidate bar of 3, and nothing
*could* promote however well attested. Fixed across both repos.

**A failed write read as a success.** The ingester guarded on
`getattr(result, "isError", False)`; `mcp` names the field `is_error` and
treats `isError` as the wire alias only, so the attribute read raised and the
`getattr` default swallowed it. The pipeline then counted the document
written *and checkpointed it done*, so a re-run skipped it. The tests agreed
with the bug because their fakes carried an `isError` attribute. The general
hazard: `getattr(obj, name, default)` on a pydantic model turns a rename into
a silent wrong answer.

**The durability gate never fired.** `drain` polled `lambo_stats` for
`log_depth == 0`, but that tool answers in rendered text, not JSON, so the
`isinstance(dict)` reading never matched. Every healthy run burned its full
60s timeout and then warned about data loss — inert exactly where it existed
to protect (Cloud Run Jobs).

## Still blocked, and it is the last thing between us and the measurement

**`lambo serve` can only run Swarm.** `PromotionPolicy` is settable only
through the in-process Rust API — not on `LamboFile`, no env override, no
`serve` flag. Lambo's own `canon/policy.rs` states the consequence: with a
single writer nothing converges and nothing is ever promoted. Mooshik's own
`serve` stamps Solo, so the M8 run on Linux looked healthier only because the
ingester J2-proxied into a running Mooshik holder; on Cloud Run there is no
holder, so it becomes the hub and runs Swarm.

Proven locally: a sqlite lambo fed one constraint on 7 days exactly 24h apart
— 7 `Derives` edges, 7 distinct `event_time` rows, 4 canonization cycles —
promoted **nothing**. Zero canonization events.

Lambo is adding the knob. Then: rebuild via Cloud Build (Docker is down
locally), execute into a fresh session, re-run M9. The corpus is already
built to produce a full ladder — one Canonical, two Venerable, two Candidate,
and the personal notes correctly staying None.

## Verification of this session's config/auth change

Mutation-tested rather than taken on trust. Each mutation applied, the named
test run, the failure confirmed, the file restored:

| Mutation | Caught by |
| --- | --- |
| `chat` passes `NoopRecall` instead of the real injector | `chat_wires_the_production_recall_injector` |
| Remove the credential-key refusal | 2 tests, incl. `setting_a_credential_key_is_refused_and_leaves_the_file_untouched` |
| Store-move guard never requires confirmation | 4 tests, incl. `a_cosmetic_dsn_edit_is_not_a_move_but_a_different_database_is` |
| Never mint a token; use a fixed bearer | `an_expired_token_is_refreshed_rather_than_reused`, `a_minted_token_never_appears_in_client_errors` |

Dropping `.with_recall` from the session composition is a **compile error**,
not a test failure — the return type demands it.

`cli.rs` hit its 1000-line cap and became `cli/`. The two `include_str!`
source pins were repointed to `cli/chat_cmd.rs` and verified to still bite,
rather than weakened into always-true checks.

256 lib + 1 integration + 142 Python tests. Clippy, fmt and the file-size cap
clean.

## Known gaps, deliberately open

* **It is not ambient.** No watcher, no proactive surfacing, no background
  reflection — `Reflect` is in the spec's architecture diagram and in no
  milestone. Recall injection now closes the in-conversation half; nothing
  yet observes the workspace on its own.
* **Embedding coverage 59.3%** on the M8 graph, so recall runs on the keyword
  leg. Diagnose on the repaired graph before assuming it persists.
* **Lambo vocabulary leaks into user-facing strings.** `stats` reports
  dead-lettered batches and write-behind depth; `recall` labels a memory
  `entity · relevance 0.29`. All in `en.toml`, so text-only — but `stats`
  probably wants splitting into an operator view and a user view rather than
  watering one down.
* **`agent.py` is imported nowhere and untested.** The GenAI SDK is the
  honest answer to the Google-framework requirement; the news MCP server now
  puts it on the live path too.
* **No `macos-latest` CI job.** Three macOS-only defects were found this
  session; without one the platform silently un-verifies.
