# Mooshik — build scope

What Mooshik itself must build, as atomic tasks with their real dependencies.

**Authority:** [docs/SPEC.md](../docs/SPEC.md). **Deadline:** 2026-09-01, 05:30 IST.
**Lambo side:** `dev-diary/lambo-for-mooshik/` on the `lambo-for-mooshik` branch of `nrynss/lambo`.

Where this doc and the spec disagree about Mooshik, the spec wins.

---

## The scoping mistake to avoid

The day plan reads as if Mooshik is day 5 — one day, after Lambo is finished. That ordering is an
artefact of writing Lambo's tasks first, and taken literally it puts every Mooshik task on the
critical path behind every Lambo task.

**Mooshik does not need finished Lambo adapters to be built.** That was true when this
paragraph was written: Lambo already shipped in-process `Memory`, SQLite, and
`FixtureEmbedder`. M0–M2 could have started against that. They did not wait.

**What actually happened:** Lambo A (Gemini) and B (Postgres) landed on
`lambo-for-mooshik`. Mooshik consumed them by git `rev` (never `branch`) at
`f90a662`, with `default-features = false` and product features
`store-postgres` + `embed-gemini`. `store-memory` and `embed-fixture` are test
doubles only. Switching Gemini later is a re-embed, so Gemini is the stamped
contract from M2. Only the bootstrap ingest and the measurement still need a
live Vertex + Cloud SQL operator run.

---

## Task graph

```
M0 ─→ M1 ─→ M2 ─→ M4 ─→ M5 ─→ M10 ─┐
      └───→ M6 ──────↗  │  ────↗    ├─→ M11
            M3 ─────────┘           │
M2 ─→ M8 ─→ M9 ─────────────────────┘

(M5, M6) ─→ M7        validate and repair the CLI grown across M2–M6

The CLI itself is not deferred to M7 — it grows with each milestone. M7 is the sweep.
M11 is last on purpose and is allowed to fail; see M11.
```

### Status (2026-08-25)

| ID | Status |
| --- | --- |
| **M0** | Built 2026-08-21, `fca2f13`. |
| **M1** | Built 2026-08-24, `79fcc2e`. P3-R8-1 (staging cleanup race) closed: fail-closed preserve, pin in `secure_path` tests. |
| **M2 + M2b** | Built 2026-08-25, `5ed3e68` / `33a5bcb`. Review round 2 **APPROVE**, zero residue (`m2-round2.md`). CI green. Live Cloud SQL + Vertex pin `bf362b3`. |
| **M3** | Built 2026-08-25, `9652f39` / `8e168f6`. Review round 2 **APPROVE**, zero residue (`m3-round2.md`). |
| **M4** | Built 2026-08-25, `73e13e1` → `5e3d113`; live-verified 2026-08-26 (`17fbe71`, `35ae812`: Gemini 3.x thought-signature echo, memory-runtime ownership). Review round 2 **APPROVE**, zero residue (`m4-round2.md`). |
| **M5** | Built 2026-08-26, `75cda1a` → `88abf81`. Review round 2 **APPROVE**, zero residue (`m5-round2.md`). Default grants: Decision 6 settled — lambo memory tools allowed, scratch prompt, rest deny. |
| **M6** | Built 2026-08-26, `071c10d` → `4613aea`. Review round 2 **APPROVE**, zero residue (`m6-round2.md`). Scratch env injection + egress redaction at the tool boundary. |
| **M7** | Built 2026-08-26, `caa8962` → `958564f`. Review round 2 **APPROVE**, zero residue (`m7-round2.md`). `recall` / `stats` commands; exit codes 0/2/1; live-verified against Cloud SQL + Vertex. |
| **M8** | Built 2026-08-26, `ingester/` subdirectory, commits through `df8d779`. Review round 2 **APPROVE**, zero residue (`m8-round2.md`). J2 proxy path + Vertex Flash extraction proven live; deployed to Cloud Run as a batch Job (IAM + deploy sequence in `ingester/README.md`). Every write now carries the document's historical `event_time` — see the event-time note under M9. |
| **M9** | Built 2026-08-26, `measurement/` subpackage, commits through `1f3217e`. Review round 2 **APPROVE**, zero residue (`m9-round2.md`). Live on the M8 graph: coverage 59.3% gated, raw precision 10/10, canonization promoted nothing (the predicted pathology, now measured). |
| **M10** | Built 2026-08-26, `2bc5665` → `f50765f`. Review round 2 **APPROVE**, zero residue (`m10-round2.md`). MCP host: configured servers, vault-ref env, `mcp.<server>.<tool>` naming, gate+redaction integrated, live-verified against real `lambo serve`. |
| **M11** | Not started. Allowed to fail. |

Lambo pin: `nrynss/lambo` git `rev = 71334f0` (`lambo-for-mooshik`). E1/E2 (path dep, then rev pin) were done as the rev pin directly; bump the SHA after a Lambo fix.

### The event-time fix (2026-08-27)

M9 measured an **empty canonical pool** — canonization promoted nothing. The
cause was not the Solo policy but a dropped input: Lambo's solo score counts
recurrence over `Interaction::about_time`, and the MCP wire had no field to
carry it, so every historical write landed stamped with the flush clock. A
decade of commit dates collapsed into one afternoon, every concept scored ~1
session against a Candidate bar of 3, and nothing *could* promote however
well-attested it was.

Fixed across both repos:

* **Lambo `71334f0`** — optional RFC3339 `event_time` on `DeriveParams` and
  `RecordActionParams`, threaded to the existing `derive_async_as` /
  `ActionRecord` paths that always supported it. `created_at` stays
  server-stamped, so F18's backdating guard is untouched.
* **Mooshik** — the ingester stamps every derive and record_action with the
  document's date (commit author date; file mtime as the weaker fallback).

Deliberately **not** exposed on Mooshik's in-process companion tools: a chat
deriving a fact asserts it about now. Historical evidence enters only through
the ingester.

**Open:** the existing M8 graph still holds NULL `event_time` rows, and the
checkpoint (keyed on content hash) will skip re-sent documents — so re-running
the ingester alone will not repair it. Re-ingest into a fresh session or
SQL-backfill; that decision is unmade. Re-running M9 afterwards is the proof:
the wrongly-rejected rate should fall from 100%.

---

## M0 — Repo and skeleton

`cargo init`, `rust-toolchain.toml` at 1.97.1 to match Lambo, a README, and the module layout.
One binary, subcommands, no cleverness. A skeleton written before there is anything to link
against gets rewritten, so this stays deliberately thin.

**Depends on:** nothing. **Done when:** `mooshik --help` runs and CI builds it.

**Status: built 2026-08-21, commit `fca2f13`.** `mooshik --help` runs; CI builds it.

Two conventions were set here that later milestones must follow:

* **User-facing strings live in TOML, not Rust.** `src/text/en.toml` holds every
  help line and message, resolved by dotted key through `text::get`. This was
  chosen for localization, but it also serves the line budget: prose stops
  counting against a file's size. A missing or empty key fails CI via test, not
  the demo.
* **File-size discipline:** soft target ~600 lines per file including tests;
  CI fails past 1000. Split at seams into directory modules, never to satisfy
  the counter. `mod.rs` stays thin.

Implementation notes worth keeping:

* clap uses the **builder API**, not derive — derive attributes only accept
  string literals, so runtime-loaded text forces the choice. Subcommands
  register there as they land.
* CI actions are pinned to **commit SHAs**, never tags (checkout v7.0.1,
  dtolnay/rust-toolchain master). Bumping one is a deliberate act. Linux CI
  installs `libdbus-1-dev` for the vault keyring (`5945671`).
* Per the warning above, no milestone directories (`companion/`, `vault/`, …)
  were scaffolded — the module table in `src/lib.rs` documents the intended
  layout instead, and each directory arrives with its milestone.

---

## M1 — Configuration and the home directory

```
~/.mooshik/
├── config.toml
├── vault          # regular file, 0600
└── logs/
```

Load, merge and validate `config.toml`; env overlay following Lambo's convention (non-empty env
wins over file, empty leaves the base intact). Create the directory on first run with the right
modes — `vault` is 0600 and that is not a detail to add later.

`mooshik.db` was in the original layout sketch as the local SQLite file. The product store
is Postgres; M2 review P3-M2-1 closed by **not** planting an empty dummy file. `HomeLayout.database`
is still a path, unused.

**Depends on:** M0.

**Status: built 2026-08-24, commit `79fcc2e` (with M6).** `mooshik init` / `config show`; env
overlay; private first-run layout. Flush interval was supposed to be picked here and landed
with M2 as 1000 ms (Lambo's default), exposed as `[daemon] flush_interval_ms`.

**P3-R8-1 closed:** failed staging directories are left in place. There is no
portable descriptor-bound rmdir, so pathname `unlinkat` after an identity check
is a same-UID race. Pin:
`staging_cleanup_does_not_remove_a_replacement_after_identity_check`.

---

## M2 — Memory in process

Wire `lambo::Memory` through `MemoryBuilder`: session, agent, store, embedder, embedding contract,
flush interval, scoring weights.

**Overridden 2026-08-25:** do not start on SQLite + `FixtureEmbedder`. Product backends are
Postgres + Vertex Gemini from M2, dim 1536, model `gemini-embedding-001`, promotion policy
**Solo**. Fixture + in-RAM store remain compiled as test doubles (`embed-fixture`,
`store-memory`). Default `cargo test` does not call Vertex or Cloud SQL. Live Cloud SQL +
Vertex were operator-run 2026-08-25 (`m2-live-gcp.md`): provision, derive, reopen-recall,
and CLI `init` all passed after enabling `aiplatform.googleapis.com` on project `mooshik`.

**M2b — publish a session endpoint.** Mooshik is a lease holder that is not a `serve`, so by
default it is unreachable and Lambo's J2 proxy cannot forward to it. Derive the address with
`SessionEndpoint::resolve`, serve Lambo's MCP surface on it, publish it with
`LeaseHolder::reachable_at()`. Roughly a morning, and it is what stops the bootstrap from being an
outage — see consequence 1 below for why the store-identity half of the path matters.

**Decided: one unified session.** Matches the single unified memory the spec promises under
*Storage*, and the
bootstrap already produces one graph. Lambo issue #4's missing session-discovery surface stays
irrelevant this month — nothing needs to enumerate sessions. Recall scoping across projects
becomes a query concern rather than a storage one.

### The consequence, which reaches into Lambo

One unified session means **the entire autobiography is one in-memory graph**. Lambo loads a
session with `load_session -> GraphSnapshot` and holds it in RAM, and its design leans on sessions
being small: issue #5 justifies SQLite's exact-cosine scan with *"Lambo is session-scoped, so n
stays small by construction"*, citing 41 concepts in one exhibit session and ~1,400 in a K=12 run.

The bootstrap corpus is 17,106 commits and 8.7M words of markdown. Whatever concept count that
extracts to, it is not 1,400. So this decision:

* puts real pressure on single-writer throughput, which is probably the first real finding this
  build produces;
* moves Lambo **F1's revisit trigger from hypothetical to likely** — exact scan over the whole
  autobiography per query is a different proposition from exact scan over one session's 41
  concepts. At 1536 dims that is ~6 KB per concept in vectors alone;
* means peak RSS at bootstrap is a number worth measuring on day 4, not discovering on day 6.

None of this changes the decision, which is right for the product. It changes what to measure, and
it means F1 should be written so the scan can be swapped without touching its callers.

**Depends on:** M1.

**Status: built 2026-08-25.** Implement `5ed3e68`, remediate `33a5bcb`, review record `ee483d3`.
Adversarial round 2 **APPROVE**, zero residue (`m2-round2.md`). 66 tests. CI green.

`mooshik init` provisions store schema only (DSN required, no Gemini/ADC). `mooshik serve` is
the J2 holder: Lambo's MCP surface, stdio, endpoint published after the lease via
`lambo::mcp::serve`. In-process `memory::open` does **not** advertise an endpoint it did not
bind. Dual-DSN authorities are compared by Lambo `store_dsn_identity` (password overlay and
omitted `:5432` are one database). Partial `[store]` / `[embedder]` tables keep product
defaults, not Lambo's Memory/1024.

---

## M3 — Companion adapter

An OpenAI-compatible `/v1` client with streaming, the message loop, and the tool-call protocol.
The slot is pluggable by design: a local model, or Gemini on Vertex, chosen by config.

Handle the unglamorous parts, because they are what makes it feel like a peer rather than a demo:
partial-stream cancellation, a tool call arriving mid-stream, context-window pressure, and a model
that returns malformed tool JSON.

**Depends on:** M0.

**Status: built 2026-08-25.** Implement `9652f39`, remediate `8e168f6`, review record
`m3-round2.md`. Adversarial round 2 **APPROVE**, zero residue. 98 tests.

Hand-rolled OpenAI-compatible `/v1` streaming client (reqwest 0.12, SSE parser, no provider SDK),
`[companion]` config with product field defaults on partial tables, `mooshik chat` REPL, tool-call
protocol with a `ToolExecutor` seam (production registers no tools), context packing that drops
old turns and calls a `RecallInjector` seam (default no-op). Default `cargo test` uses an
in-process HTTP/SSE mock and does not call a live model. Record:
`dev-diary/adversarial-review/m3-implementation.md`.

---

## M4 — The tool surface

`lambo_recall`, `lambo_derive`, `lambo_stats`, and the scratch script runner.

**Decided: `run_scratch_script` is in scope.** It gives M6's injection path a consumer and makes
egress redaction demonstrable — a script echoing `$TOKEN` is the exact failure the two-store design
claims to prevent, and it is the only tool in this build that shows autonomy rather than recall.
Carries a sandbox, a hard timeout, and the permission-prompt path.

Out: `delegate_to_coder`. Also `search_web` and `fetch_page` **as hand-written Rust tools** — they
come back through M10 as configured servers instead, which is the whole argument for M10.

Delegation stays out on scope, not on difficulty. A coding agent is a subprocess with a prompt and
a working directory, so when it lands it is one tool behind the existing gate and sandbox — no
bridge, no sidecar. Nothing in M4 needs to anticipate it.

### Lift these from Lambo rather than writing them

Mooshik exposes the *same* lambo tools its MCP server does, just backed by in-process `Memory`
instead of JSON-RPC. The schemas are therefore already written, already bounded, and already
survived review. In `~/work/lambo/src/mcp/`:

| What | Where | Why it is worth taking |
| --- | --- | --- |
| `RecallParams`, `DeriveParams`, `WireConcept`, `WireResource`, `RecordActionParams` | `server.rs` (118, 159, 140, 176, 180) | Plain `serde` + `schemars`, no rmcp coupling — checked. `deny_unknown_fields` plus length and range caps already chosen. These *are* M4's tool schemas. |
| `SecretToken` and its hand-written `Debug` | `serve.rs` 207, 233 | A newtype that refuses to print its own value. Exactly the shape M6 wants for `secret://`, so a vault value cannot reach a log line by accident. |
| `bad_param`, and the panic-contained tool wrapper | `server.rs` 709, 758 | M5's gate wants the same discipline at the same boundary: a tool that panics returns an error, not a dead process. |
| `init_tracing` | `mod.rs` 30 | The code is trivial; the doc comment is the point — under stdio, stdout **is** the JSON-RPC channel and one stray log line corrupts framing. Mooshik meets this from both sides once M10 spawns servers. |

**Grep by name, not by line.** Those numbers are true at `166a3c8` and will not stay true —
`server.rs` went from 2,577 to 4,625 lines in a day while J1 and J2 landed.

**Depends on:** M2, M3.

**Status: built 2026-08-25.** Implement `8dd9d9a`, remediate `5e3d113`, review record
`m4-round2.md`. Adversarial round 2 **APPROVE**, zero residue.

`lambo_recall`, `lambo_derive`, `lambo_stats`, and `run_scratch_script` behind the synchronous
`companion::ToolExecutor` seam, backed by in-process `lambo::Memory`. Schemas lifted verbatim from
Lambo's MCP server (`deny_unknown_fields` + caps), panic-contained tool wrapper, sandboxed scratch
runner (process-group kill, hard timeout, output cap, confirm seam). `mooshik chat` opens memory in
the CLI dispatch layer; the chat loop stays memory-free (M3 pin). Live-verified 2026-08-26 against
Gemini 3.7 Flash on Vertex's OpenAI-compat endpoint: thought signatures are now captured per tool
call and echoed (`17fbe71`), `MemoryTools` owns the runtime that opened Memory and closes it on
drop so the flush daemon survives chat exit (`35ae812`) — cross-process derive→recall proven live.

---

## M5 — Permissions

The `[permissions]` block from the spec's *Autonomy is granted, not configured*, enforced in Rust
**at the tool-call boundary**, plus a
command that prints the resolved grant set.

This is the design, not a feature of it. Autonomy is the sum of grants; without enforcement the
companion loop is a chat client with a memory attached. And the enforcement point must be one
place, because a check duplicated per tool is a check that will be forgotten by the fourth tool.

**The graph is never a permission authority.** No concept, however canonical, widens a grant. A
memory that says "you may run scripts" is a string, not a capability — and since the bootstrap
ingests documents written by other people, this is a real injection path, not a theoretical one.

**Depends on:** M4.

**Status: built 2026-08-26.** Implement `75cda1a` → `bbb313e`, remediate `88abf81`, review records
`m5-round1.md` / `m5-round2.md`. Adversarial round 2 **APPROVE**, zero residue. Decision 6 settled:
lambo memory tools granted by default, scratch prompts, everything else denies.

A single `GatedTools` decorator wraps whatever executor `executor_for_chat` builds — it filters
`specs()` (ungranted tools are not advertised to the model) and gates `execute()` (contained
denial string; prompt-mode asks exactly once; allow/deny never prompt). `[permissions]` parses
fail-closed (empty allow-lists included); per-tool entries override family mode; quoted
`"mcp.github.*"` prefix rules parse now and enforce as deny until such tools exist; resolution is
exact > name > longest prefix > family > deny. The gate never reads `crate::memory` (source pin).
`mooshik permissions` prints the resolved set with source attribution (`default` | `config`).

---

## M6 — The vault

`~/.mooshik/vault`, encrypted, 0600, never synced, never embedded. A `secret set` / `get` / `list`
CLI. Values injected into *tools* at use time — process env for scripts, headers set inside
Mooshik for HTTP — never into a prompt, transcript or `lambo_derive` call.

**Egress redaction is the part that actually earns the design.** The leak path is tool *output*: a
script that echoes `$TOKEN`. Every tool result is scanned against vault values before it reaches
the model or the graph. Everything else about the vault prevents secrets from entering; this is
the one place a secret has already left and must be caught.

**Decided: OS keyring default, Argon2id passphrase fallback.** Linux
`linux-native-sync-persistent` + `crypto-rust`; macOS `apple-native`. Unattended start is
possible. CI installs `libdbus-1-dev` because that Linux feature links libdbus.

**Depends on:** M1.

**Status: vault file + CLI built 2026-08-24, commit `79fcc2e` (with M1).** `secret set` /
`get` / `list`; v2 header is AAD; keyring default and passphrase fallback.

**Built 2026-08-26.** Implement `071c10d` → `013c3cc`, remediate `4613aea`, review records
`m6-round1.md` / `m6-round2.md`. Adversarial round 2 **APPROVE**, zero residue.

Injection: `[tools.scratch.env]` maps env-var names to secret names (fail-closed parse); values
resolve through `vault.get` per run, after confirm and before spawn, and exist as plaintext only
inside the `Command::env` call — a missing secret aborts the script before it starts.
Egress: `RedactingTools` sits in the chat composition under the M5 gate and scans every tool
result against all vault values — literal **and** JSON-escaped forms (round 1 found escaped
secrets crossing unredacted) — post-execute, pre-history, so neither the transcript nor the graph
ever holds a value. An unopenable vault never blocks chat; a non-empty env table without a vault
fails the script start closed. Known deliberate limits documented at the boundary: output-cap
truncation can split a token, and args are deliberately unscanned (scripts receive `$TOKEN`
references, never values).

---

## M7 — The CLI: validate and repair

**The standing rule stays: nothing in M2–M6 is done until it has a CLI surface.** Every milestone
lands with the commands that drive it — `mooshik chat`, `recall`, `stats`, `secret set/get/list`,
`permissions`, `config` — written as that milestone is written, not retrofitted afterwards. Built
alongside, it costs each milestone an hour; built at the end, it costs a day nobody has.

**M7 is the sweep that follows.** One pass over the whole surface by an implementation agent, with
authority to change what it finds. Six milestones written days apart produce six dialects: flags
named three ways, errors that report a Rust type instead of a cause, help text that describes a
flag that moved. Nobody notices from inside a single milestone, and everybody notices in the demo.

**The floor, even if nothing else changes: the messages have to be right.**

* Every error says what failed, why, and what to do next. "Not found" is not a message.
* No `Debug` output, no Rust type names, no `unwrap` panics reaching a user.
* **No vault value can appear in an error string**, including inside a wrapped source error. This
  is `SecretToken`'s job (see M4's lift table) and M7 is where it gets verified rather than assumed.
* One voice and one casing across every command.
* Every example in `--help` actually runs as written.

**Decided: a chat with context.** A persistent conversational loop, not one-shot subcommands.
Ambience and continuity are the product's claim, and a one-shot command cannot demonstrate either.
The other commands stay one-shot; `chat` is the one that holds a session.

This decision does more work than it looks like it does — see below.

**Depends on:** M5 and M6 — it audits the surface those complete. `chat` itself needs M3.
**Done when:** the demo can be driven end to end from the CLI with the TUI not existing, and a
reader who has never seen the project can get from `mooshik --help` to a recall without asking
anyone.

**Status: built 2026-08-26.** Implement `caa8962` → `4dbf798`, remediate `958564f`, review
records `m7-round1.md` / `m7-round2.md`. Adversarial round 2 **APPROVE**, zero residue.

The sweep: `recall` / `stats` one-shot commands over in-process memory (defaults top_k 5,
max_tokens 200, depth 0); exit codes **0 ok / 2 user error / 1 internal** decided once in a
chain-walking classifier; ONE terminal error path that prints only the top-level en.toml message —
wrapped sources (LamboError can carry DSN material) never print, proven by a subprocess pin whose
fixture chain plants fake credentials; no vault value reachable from any error string, pinned
behaviorally. Lease conflicts surface as their own user error naming the holder and the takeover
remediation instead of a generic backend message; chat's graceful executor close runs on the
failure path too. Live-verified against Cloud SQL + Vertex Gemini: every command exit-0, and a
chat-driven derive was recalled by a separate `mooshik recall` process through the real stack.
Known documented limits: recall-during-held-lease classification is pinned at variant level;
help-example parser tokenizes raw TOML bytes.

---

## What a persistent chat loop forces

Three consequences, and the first is a collision that has to be resolved before M8 runs.

### 1. Two writers on one session — J2 changes the answer, but not for free

**Updated 2026-08-20 after pulling `lambo-for-mooshik`. The old resolution here — stop Mooshik
while the bootstrap runs — is superseded, and the thing that replaces it is a task, not a freebie.**

The lease is unchanged: one writer per session, store-enforced, no preemption. What changed is
what happens to the loser. Lambo's **J2** makes a refused `lambo serve` bind nothing and instead
*proxy* to the holder — it reads the winner's address out of the new `session_leases.endpoint`
column, connects, and forwards every call. Full read and write for every client, real
read-your-writes, no client config change. First process to start becomes the hub.

**The catch, and it lands squarely on M2.** The endpoint is published by the *holder*, opt-in, via
`LeaseHolder::reachable_at()` — and `serve` is what calls it. `store/lease.rs:212` names the gap
out loud: a row carries no endpoint when the holder is *"a writer that is not a serve"*. Mooshik
holding `lambo::Memory` in process is exactly that writer. Left alone it wins the lease, publishes
no address, and the bootstrap's `serve` loses to a holder it cannot reach — refused, same as
before, except now the failure looks like a bug in J2 rather than a known constraint.

So M2 gained a task: **Mooshik binds a session endpoint and publishes it like a holder does.**
**Landed:** `mooshik serve` calls `lambo::mcp::serve`, which derives `SessionEndpoint::for_store`
and publishes at acquire. `memory::open` (in-process, not a holder) does not publish.

* Derive the path with `SessionEndpoint::resolve` rather than inventing one. It is keyed on the
  session id **and the store's canonicalized identity**, and that discriminator is load-bearing —
  J2-R1-2 records two graphs landing on one socket when a relative `path = "./lambo.db"` was
  hashed verbatim, at which point one holder's stale-socket cleanup unlinked the other's live
  socket and a proxy forwarded writes into the wrong graph.
* Serve Lambo's MCP surface on it, so an arriving proxy finds what it expects.
* Publish with `reachable_at()` at acquire time.

What this buys: the bootstrap ingester stops being an outage. Mooshik stays up while a decade of
history loads through it, which is a materially better demo than a companion that has to be shut
off to be filled.

Still worth saying in the write-up — an always-on companion and a bulk importer genuinely contend
for one memory. The finding is just no longer "fail-closed is the right answer"; it is that
process-level locking was reading as agent-level outage, and the fix was to make the loser useful.

### 2. A crash loses the write-behind tail — and J3 gives it a dial

The lease module is explicit that a lease expiring proves nothing about durability: *"the tail
lived in the crashed process's in-RAM log and died with it."* A long-running chat process
accumulates unflushed mutations between flush intervals, so a crash silently loses the most recent
memory — the part the user just created and is most likely to notice missing.

The flush interval is a product setting, not a performance knob. Picked with M2: default
1000 ms (`[daemon] flush_interval_ms`); zero fails closed.

**J3 adds the other half.** Writes are acknowledged before the embedder runs — a warm
`lambo_derive` is 27ms of which 22–25ms is embedding — and the ack carries a **receipt id**.
Outcomes come back piggybacked on that agent's next tool response, and the receipt is opt-in
synchrony: a caller that needs its write applied can wait on it. So Mooshik gets read-your-writes
where it matters without paying the embedder on every derive. Note the rule J3 states: `derive`
and `record_action` may ack async, **`lambo_reserve` may not** — its result *is* the caller's next
action.

### 3. Context pressure becomes the demo, not a bug

A persistent conversation will exceed the companion's context window. The wrong answer is
truncation or summarization. The right one is the product thesis: **the model does not remember,
the graph does.** Old turns leave the window and return through `lambo_recall` when they are
relevant.

That is the single most compelling thing the demo video can show, and it only exists because the
loop is persistent. It should be designed in M3 rather than patched when the window fills.

---

## M8 — The bootstrap ingester

An ADK agent on Cloud Run that walks this machine's history, has Gemini Flash extract concepts at
volume, and writes through `lambo serve` over MCP.

**Not "as the single writer" any more.** Post-J2 its `serve` loses the lease to a running Mooshik
and proxies into it, provided M2b published an endpoint. That is the intended path, not a
workaround: one graph, one hub, and the companion stays up while the corpus loads.

**Decided: subdirectory of this repo.** Mooshik is public; the ingester lives here — one link
in the submission, one clone for a judge.

Ingest rules: allowlist by extension rather than denylist, a
secret scanner over every candidate document with a hit dropping the document rather than
redacting it, and **no `git diff` content** — commit messages and metadata only, because diffs are
where secrets hide.

**Depends on:** M2, and Lambo's A and B. **Those dependencies have landed.** M8 itself has not.

**Status: built 2026-08-26.** Implement → remediate `df8d779` (branch `m8-ingester`), review
records `m8-round1.md` / `m8-round2.md`. Adversarial round 2 **APPROVE**, zero residue.

`ingester/` — Python package (google-genai extraction, ADK-shaped agent module, official `mcp`
stdio client). Pipeline: extension allowlist walk (symlinks never cross the root) → secret
scanner dropping whole documents pre-chunking (path-only logging) → chunker with checkpoint
resume keyed on content hash (documented at-least-once window) → Gemini Flash concepts → writer
spawning `lambo serve`, whose child env is a targeted ALLOWLIST (vault passphrase canary pinned
absent). Provenance: `lambo_record_action` resources + derive `parent_of`. Live-proven: J2 proxy
path (ingester's serve proxied into a running `mooshik serve` holder), 14 concepts written and
recalled via fresh-process CLI; dropped PEM content absent from the graph (SQL-verified).
Offline pytest suite (36) in regular CI. **Deployed to Cloud Run as a batch
Job 2026-08-26** (`ingester` in project `mooshik`, us-central1): auth-proxy
tunnel to Cloud SQL, ADC-attached service account for Vertex inference;
execution verified end-to-end via fresh-process recall.

---

## M9 — The measurement

Sample N raw extractions and N canonical facts, hand-verify both against source documents, report
precision of each with the difference and an interval — **and report what canonization wrongly
rejected**, not only what it correctly rejected. A filter that promotes nothing has perfect
precision and no value.

This is the intellectual contribution and the reason the project is not a plumbing exercise. It
is also the first thing to get squeezed, so the sampling harness should exist before the corpus
does.

**Check embedding coverage before reporting anything about recall.** Lambo's own dogfood run found
*92 of 100 concepts with no durable embedding* — semantic memory mostly blind, while recall still
returned plausible-looking results off the keyword leg. A precision number measured over a corpus
in that state is measuring the wrong system. Read `DOGFOOD-FINDINGS.md` before designing the
sample, and report coverage alongside precision.

Two more from the same log worth reading rather than rediscovering: canonical=0 on a real corpus
(which is C's motivating evidence, and matches what M2's SoloPolicy note already predicts), and
G's finding that a flat `RECENT_SCORE` floor can erase a correct cosine ranking — so recall
quality is G's to calibrate, not something to tune from inside Mooshik.

**Depends on:** M8.

**Status: built 2026-08-26.** Implement → remediate `1f3217e` (branch `m9-measurement`), review
records `m9-round1.md` / `m9-round2.md`. Adversarial round 2 **APPROVE**, zero residue.

`measurement/` — Python subpackage, `python -m measurement sample/grade/report`. Seeded per-pool
deterministic draws from the live Cloud SQL graph over a fakeable connection seam; grades keyed by
node id with merge-on-save surviving Ctrl-C (save in `finally`) and EOF; fan-out dedup so a concept
extracted from two documents is never counted twice; markdown report with Wilson 95% intervals —
raw-extraction precision, canonical-fact precision, **wrongly-rejected rate** (the non-canonical
slice of raw extractions), and embedding coverage reported FIRST with an explicit <90%
keyword-leg warning gate. Live on the M8 graph: coverage 59.3% (gate fired), raw precision
10/10 [0.722, 1.000], canonical pool empty — **canonization promoted nothing**, so every true
extraction registers as wrongly rejected: the pathology this milestone predicted, now measured
instead of asserted. 39 offline tests; measurement pytest added to regular CI.

---

## M10 — The MCP host

Without this the tool surface is seven tools welded into the binary, and every new capability is a
Rust PR. With it, capability is configuration: a search server restores `search_web` and
`fetch_page` without either being written here, and `[mcp_servers.*]` in `config.toml` covers
github, obsidian and whatever else without touching the tool code.

`rmcp` is already a Lambo dependency for its MCP **server**; this is the **client** half of the
same crate.

Do not expect to lift code from `src/mcp/`. It is the other half of the protocol —
`#[tool_router]`, `ServerHandler`, inbound HTTP transport, rate limiter, auth token — and none of
it is a client. What M4 lifts from there is listed under M4; M10's own plumbing is new. The one
exception worth reading first is `proxy.rs`, which is a client: it dials a socket and forwards
tool calls, which is structurally what M10 does to every configured server.

**Copy Lambo's dependency line carefully.** It pins `default-features = false` and its comment says
that is required rather than stylistic: rmcp's optional `reqwest` is `^0.13.2`, and a repo pinning
reqwest 0.12 compiles it twice otherwise. Mooshik pins reqwest for the companion client, so it
walks into the same trap. Features here are `client` and `transport-child-process`.

### Two constraints, or this breaks things already decided

**Exposure is an allowlist, per server.** Aggregation's default behaviour is to hand the model
every tool every server offers, and the spec's *companion slot* trims the surface to stop a small local
model misrouting. Config names which tools are exposed; the companion still sees roughly eight,
not forty. Without this, M10 breaks the local-companion premise rather than extending it.

**Server credentials come from the vault.** A server's `env` holds vault references, resolved at
spawn time — never literal tokens. A literal would put a live credential in a readable config file,
which is the exact thing M6 exists to prevent. This is also the better vault demonstration: a real
credential reaching a real tool and never a prompt, rather than a script echoing a variable.

MCP tools are tools: they pass M5's gate like everything else, which means the grant syntax needs
to name them (`mcp.github.*`). That is the one place M5 has to be written knowing M10 exists.

**Depends on:** M4, M5, M6.

**Cost, honestly:** process lifecycle, stdio JSON-RPC, discovery, and reconnect-on-crash is a real
day even with rmcp, and it competes with M8 and M9. If the clock forces a choice, this is the
better thing to cut than M9 — M9 is the finding, this is plumbing that will still be here in
September.

**Status: built 2026-08-26.** Implement `2bc5665`, remediate `f50765f`, review records
`m10-round1.md` / `m10-round2.md`. Adversarial round 2 **APPROVE**, zero residue.

`[mcp_servers.<name>]` (command/args, env as vault secret refs resolved at spawn,
per-server `expose` allowlist, empty-expose inert) → `McpTools` exposes
`mcp.<server>.<tool>` tools into the same `GatedTools(RedactingTools(.))` chain;
lazy spawn, bounded one-respawn reconnect, per-call timeout firebreak (a hung
child frees the worker), dotted server keys rejected at load. `search_web` /
`fetch_page` and github/obsidian now arrive as configured servers, not Rust
code. rmcp 3.1.2 `default-features=false` + `client`/`transport-child-process`;
reqwest still 0.12 once. Live-verified against `lambo serve` as a real MCP
server (unique session): MCP-hosted derive landed and was recalled.

---

## M11 — The TUI

**The intended face of the product, and deliberately the last thing built.**

An always-on companion wants a surface you leave open: the conversation, what the graph is doing
underneath it, what was recalled and why, what is pending approval. That is a `ratatui` job, and it
is what Mooshik should look like.

It goes last for one reason. Every capability is already reachable from the CLI by the time this
starts, so the TUI is a second view over finished behaviour rather than the only way to reach it.
If it lands, the product has its face. If it runs out of clock, is unstable, or simply looks worse
than the CLI, it is dropped on the day with nothing lost — no capability lives only here.

That safety only holds if M7 was honoured. A TUI built last on top of a half-built CLI is not a
final task, it is the surface arriving late; the whole point of the ordering is that this milestone
can fail.

**Depends on:** everything shipped. **Done when:** the demo runs in the TUI — or the demo runs in
the CLI and this is cut, which is a decision rather than a failure.

**Status: not started.** Allowed to fail.

---

## Decisions needed, in the order they bite

| # | Decision | Blocks | Cost of deciding late |
| --- | --- | --- | --- |
| ~~1~~ | ~~One session or many~~ | — | **Decided: one unified session.** See M2. |
| ~~2~~ | ~~`run_scratch_script` in scope~~ | — | **Decided: in scope.** See M4. |
| ~~3~~ | ~~Vault key: keyring or passphrase~~ | — | **Decided: keyring default, passphrase fallback.** Unattended start is possible. See M6. |
| ~~4~~ | ~~Chat loop or one-shot CLI~~ | — | **Decided: a chat with context.** See M7. |
| ~~5~~ | ~~Ingester repo location~~ | — | **Decided: subdirectory of this repo** (Mooshik is public). See M8. |
| ~~6~~ | ~~Default grant set for the demo~~ | — | **Decided: lambo memory tools allowed, scratch prompts, rest denies.** Landed with M5. |
| ~~7~~ | ~~Where the surface effort goes~~ | — | **Decided: CLI throughout, a repair sweep at M7, TUI last.** See M7 and M11. |
| ~~8~~ | ~~Companion loop: framework or hand-rolled~~ | — | **Decided: hand-rolled.** See below. |

All eight settled. Product backends (Postgres + Gemini, not SQLite + fixture) were decided when
M2 started and are not a numbered row here.

**On decision 8**, since it will be proposed again: the companion targets exactly one wire format,
OpenAI-compatible `/v1`, which every candidate endpoint already speaks. A framework's value is an
abstraction over provider variation Mooshik does not have, and the parts of M3 that are actually
hard — partial-stream cancellation, a tool call arriving mid-stream, context pressure answered by
recall rather than truncation — are not what a framework hands you. The deciding argument is M5:
the permission gate belongs at the tool-call boundary, and owning the loop is how that is held
outright instead of depending on someone else's hook granularity. The dependency taken is an SSE
parser, not a harness.
