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

---

# The repair run, 2026-08-27

## Before, measured (session `mooshik`, the graph M9 reported on)

```
44 concepts   |  None 38, Candidate 6, Venerable 0, Canonical 0
event_time    |  0 of 16 interactions stamped
embedded      |  26/44 = 59.1%      (M9 reported 59.3% — same graph)
```

Every interaction NULL is the first root cause rendered in SQL.

## After the event-time fix (session `mooshik-v2`, partial run)

```
531 concepts  |  None 500, Candidate 31, Venerable 0, Canonical 0
event_time    |  62 of 62 interactions stamped — 100%
embedded      |  341/531 = 64.2%
```

Event time is carried end to end, and canonization is genuinely running:
real `None → Candidate` transitions in the logs. That half is closed.

## The third root cause, found by the live run

Nothing exceeds Candidate, and the reason is neither of the first two:

```
CanonizationCycleFailed: canonization cycle failed after 3 committed
transition(s): error occurred while decoding column "coverage": mismatched
types; Rust type `f64` (as SQL type `FLOAT8`) is not compatible with SQL
type `NUMERIC`
```

Stage 1 commits, then the cycle dies reading `coverage` — the Stage-2
(`Candidate → Venerable`) gate. **Stage 2 has never once succeeded on the
Postgres adapter**, which is also why the *old* graph capped at Candidate.

Cause, verified against the live instance rather than inferred:
`INTERACTION_SPAN_SQL` computes coverage with `extract(epoch FROM ...)`, and
**PostgreSQL 14 changed `extract` to return `numeric`** instead of double
precision. The instance is 16.14. Every arm of the `CASE` is numeric —
including the bare `0.0` / `1.0` literals — so `try_get::<f64>` fails
unconditionally.

```
postgres: 16.14
extract(epoch FROM interval) -> numeric
literal 0.0                  -> numeric
lambo's coverage expression  -> numeric
```

`distinct_count` and `blast_radius` are `count(*)` → `bigint` → `i64`, and
are fine. Handed to Lambo as a one-cast fix.

**Why it survived this long, and the lesson.** The offline ladder test climbs
cleanly to Canonical on **SQLite**, whose type affinity accepts the value as
a float. The identical corpus caps at Candidate on Postgres. A test that runs
only against SQLite or the in-memory store cannot catch an adapter-specific
decode bug — and the product store is the one that was broken.

## Two operational findings

* **The Cloud Run task timeout was 600s.** 54 documents through Gemini Flash
  needs longer, so the first execution was killed mid-corpus — which is why
  only 62 interactions landed in `mooshik-v2`. Raised to 3600s.
* **ADC and the gcloud CLI are different credentials here.** Secret Manager
  and Cloud Run worked (CLI credential); the Cloud SQL proxy authorizes via
  ADC and got a 403 that reads like a missing instance permission. Pass
  `--token "$(gcloud auth print-access-token)"` to use the working one.
* **`.dockerignore` listed `/src`** — correct while the image shipped only
  Python, fatal once it compiles Mooshik. And without a `.gcloudignore`,
  gcloud falls back to `.gitignore`; `target/` alone is 7.1G.

## Where it stands

Two of three root causes closed and verified in production. The third is a
one-line cast in Lambo. Once it lands, canonization climbs the rest of the
ladder on the **existing** graph — the daemon re-evaluates every cycle, so it
needs a rebuild and a short run, not another full Gemini pass.

---

# Outcome, and a fourth cause behind the other three

## What the repair achieved

```
                        before (mooshik)   after (mooshik-v3)
concepts                      44                 325+
event_time stamped          0 / 16            120 / 120
distinct corpus days           -              7 (21-27 Aug)
Venerable                      0                  2
Candidate                      6                 42
```

Three causes closed and verified in production:

1. historical `event_time` reaches the graph (lambo `71334f0` + this repo),
2. the bootstrap writer runs Solo, not Swarm (lambo `9aa8939`; and the image
   now ships one binary so the serve child cannot drift),
3. **Stage 2 decodes `coverage` and promoted to Venerable on Postgres for
   the first time** (lambo `b829099`).

Plus two of our own: a failed MCP write read as success, and a durability
gate that first never fired and then watched the wrong depth.

## The fourth cause: an LLM extractor does not repeat itself

The corpus facts still do not reach Canonical, and no further bug is
responsible. The corpus was built on the assumption that a **verbatim** source
sentence yields an identical extracted concept. It does not. One sentence,
repeated across the week, entered the graph as three separate nodes:

```
"The Windpipe ring never holds more than 512 in-flight messages."
"The Windpipe ring has a maximum capacity of 512 in-flight messages."
"The Windpipe ring has a maximum capacity of 512 in-flight messages;
 overflow writers block instead of dropping messages."
```

Each carries one supporting interaction on one day. No merge, so no
recurrence, so nothing can climb — independent of every fix above.

The tell is in what *did* earn standing: both Venerable concepts are
`document:file:...` resources. Those have deterministic, machine-generated
names, so they match exactly, merge, and accumulate. The only part of the
corpus with stable identity is the only part that climbed.

All three copies are embedded, so this is not simply missing vectors. Either
the concept had no embedding **at derive time** — writes ack before the
embedder runs, which is J3's design — leaving exact-text matching as the only
fallback, or the semantic threshold is stricter than these paraphrases.
Unconfirmed; worth settling before prescribing a fix.

**The general shape, if it holds:** a bulk ingest may be structurally unable
to merge semantically, because concepts are created before their embeddings
exist. Then a bootstrap can only merge on exact text, and an LLM extractor
will not give you exact text. Recurrence — and therefore earned memory —
would depend on the extractor emitting stable identity for a repeated fact.

## Decision

Accepted as-is: Mooshik does not lean on canonization. Recall runs off
embeddings and the keyword leg, not canonization status.

The residue is a **recall** concern rather than a canonization one: three
paraphrases of one fact can occupy three slots of a `top_k` of 5, so
duplicates crowd the window. Worth weighing when recall quality is measured,
not before.

## Each defect this session was a false green

Not one of the six was a crash. A wire silently dropping a field; a policy
defaulting to one that cannot work for a single writer; a decode that fails
only on the store the product actually uses; a write error read as success; a
durability gate reporting durable over an incomplete graph; and a corpus whose
recurrence was an assumption nobody had measured. Each looked healthy, and
each masked the next — which is why they could only be found one at a time,
in that order, against the live store.

---

# The clean run, 2026-08-29

Graph wiped (6292 rows), image rebuilt from merged `main` at lambo
`4c6fc93`, one ingest into session `mooshik`. First run with all four causes
fixed at once, so the first numbers that measure the system rather than a
defect.

```
                    M8 graph      clean run
concepts                  44            748
event_time stamped      0/16        106/106   across 7 days
embedding coverage     59.1%          85.8%
shadow twins               -              0   (was 170 on mooshik-v3)
ingest outcome             -      succeeded   (no timeout, no failure)
```

**The `parent_of` fix is the headline.** Zero shadow twins: the unembedded
`Entity` duplicate that used to accompany every concept is gone. Coverage now
reflects the embedder instead of a structural defect — which settles what
M9's 59.3% warning was firing on. Not embedder lag. Duplication nobody had
found.

**The ladder still stops at Candidate**, and no bug is responsible:

```
windpipe 512     8 nodes, best 3 days -> Candidate
zephyr 40ms     14 nodes, best 2 days -> Candidate
quillstone NAS   8 nodes, best 1 day  -> None
```

An LLM extractor paraphrases, so one verbatim sentence becomes many concepts
and recurrence spreads across them rather than accumulating on one. Accepted:
Mooshik does not lean on canonization.

**It is NOT a recall concern. That claim was wrong, and measuring killed it.**

The "fourteen nodes for one fact" figure counted concepts matching a text
*pattern* — every extraction mentioning Zephyr's quantum. Those are not
restatements. They are distinct facts pulled from different documents:
round-robin scheduling, trivial-task overhead, why a fixed 40 ms is
inefficient. Text-pattern counting conflated "about the same subject" with
"says the same thing"; the vector space does not.

Measured on the clean graph with pgvector, over 40 sampled concepts:

```
nearest-neighbour distance    median 0.031
mean distance to everything   median 0.353      -> 11.5x separation
concepts within 0.02 (true paraphrase radius)   0.53 on average
```

Half a concept, on average, sits within genuine paraphrase distance. A
`top_k` of 5 is not eaten by duplicates. Nearest neighbours of a seed are
topically coherent and individually distinct — and the personal seed proves
the domains separate: querying near the novel returns the gig, five-a-side
football, a friend between jobs, the postponed visits, cleanly apart from the
Zephyr and Cobalt Lantern clusters.

So the paraphrase behaviour costs **canonization** (recurrence spreads across
nodes instead of accumulating, which is why nothing climbs past Candidate)
and costs recall nothing. Since Mooshik leans on recall and not on
canonization, the accepted trade is cheaper than this document first claimed.

## What the whole episode was

Six defects, and **not one was a crash.** A wire dropping a field; a policy
defaulting to one unusable for a single writer; a decode failing only on the
store the product runs on; a write error read as success; a durability gate
reporting durable over an incomplete graph; and provenance wiring minting a
twin for every concept. Each looked healthy, each masked the next, and
several were agreed with by the offline suite. They could only be found one
at a time, against the live store, in that order.
