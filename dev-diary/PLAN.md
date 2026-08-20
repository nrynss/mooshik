# Mooshik — build scope

What Mooshik itself must build, as atomic tasks with their real dependencies.

**Authority:** `../scratch/PRODUCT_SPEC.md`. **Clock:** `../scratch/hackathon.md` (submission
2026-09-01, 05:30 IST). Both are gitignored and do not travel with this repo — carry them between
machines yourself.
**Lambo side:** `~/work/lambo/dev-diary/lambo-for-mooshik/` on branch `lambo-for-mooshik`.

Where this doc and the spec disagree about Mooshik, the spec wins.

---

## The scoping mistake to avoid

The day plan reads as if Mooshik is day 5 — one day, after Lambo is finished. That ordering is an
artefact of writing Lambo's tasks first, and taken literally it puts every Mooshik task on the
critical path behind every Lambo task.

**Mooshik does not need finished Lambo adapters to be built.** Lambo today already ships an
in-process `Memory`, a SQLite store, and a deterministic `FixtureEmbedder` behind
`embed-fixture`. M0 through M6 can be built and tested against that, and the Gemini embedder and
Postgres store swap in underneath as configuration once they land. Only the bootstrap ingest and
the measurement genuinely need them.

So: start M0–M2 in parallel with Lambo's A and B, not after.

---

## Task graph

```
M0 ─→ M1 ─→ M2 ─→ M4 ─→ M5 ─→ M11 ─┐
      └───→ M6 ──────↗  │  ────↗    ├─→ M10
            M3 ─────────┘           │
M2 ─→ M8 ─→ M9 ─────────────────────┘

M7 is not a node. The CLI grows with M2–M6; see M7.
M11 is an id, not a position: after M6, before M10.
M10 is last on purpose and is allowed to fail; see M10.
```

---

## M0 — Repo and skeleton

`cargo init`, `rust-toolchain.toml` at 1.97.1 to match Lambo, a README, and the module layout.
One binary, subcommands, no cleverness. A skeleton written before there is anything to link
against gets rewritten, so this stays deliberately thin.

**Depends on:** nothing. **Done when:** `mooshik --help` runs and CI builds it.

---

## M1 — Configuration and the home directory

```
~/.mooshik/
├── config.toml
├── mooshik.db
├── vault
└── logs/
```

Load, merge and validate `config.toml`; env overlay following Lambo's convention (non-empty env
wins over file, empty leaves the base intact). Create the directory on first run with the right
modes — `vault` is 0600 and that is not a detail to add later.

**Depends on:** M0.

---

## M2 — Memory in process

Wire `lambo::Memory` through `MemoryBuilder`: session, agent, store, embedder, embedding contract,
flush interval, scoring weights.

Build against **SQLite plus `FixtureEmbedder`** first. That combination needs no GPU, no network
and no cloud account, so M3–M6 can be developed and tested offline. Gemini and Postgres become a
config change, not a rewrite — which is the whole point of the `GraphStore` seam.

**M2b — publish a session endpoint.** Mooshik is a lease holder that is not a `serve`, so by
default it is unreachable and Lambo's J2 proxy cannot forward to it. Derive the address with
`SessionEndpoint::resolve`, serve Lambo's MCP surface on it, publish it with
`LeaseHolder::reachable_at()`. Roughly a morning, and it is what stops the bootstrap from being an
outage — see consequence 1 below for why the store-identity half of the path matters.

**Decided: one unified session.** Matches spec §3.3's single autobiographical memory, and the
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

* puts real pressure on the single-writer throughput question the hackathon doc already flags as
  "probably the first real finding";
* moves Lambo **F1's revisit trigger from hypothetical to likely** — exact scan over the whole
  autobiography per query is a different proposition from exact scan over one session's 41
  concepts. At 1536 dims that is ~6 KB per concept in vectors alone;
* means peak RSS at bootstrap is a number worth measuring on day 4, not discovering on day 6.

None of this changes the decision, which is right for the product. It changes what to measure, and
it means F1 should be written so the scan can be swapped without touching its callers.

**Depends on:** M1.

---

## M3 — Companion adapter

An OpenAI-compatible `/v1` client with streaming, the message loop, and the tool-call protocol.
The slot is pluggable by design: a local model, or Gemini on Vertex, chosen by config.

Handle the unglamorous parts, because they are what makes it feel like a peer rather than a demo:
partial-stream cancellation, a tool call arriving mid-stream, context-window pressure, and a model
that returns malformed tool JSON.

**Depends on:** M0.

---

## M4 — The tool surface

`lambo_recall`, `lambo_derive`, `lambo_stats`, and the scratch script runner.

**Decided: `run_scratch_script` is in scope.** It gives M6's injection path a consumer and makes
egress redaction demonstrable — a script echoing `$TOKEN` is the exact failure the two-store design
claims to prevent, and it is the only tool in this build that shows autonomy rather than recall.
Carries a sandbox, a hard timeout, and the permission-prompt path.

Out: `delegate_to_coder`. Also `search_web` and `fetch_page` **as hand-written Rust tools** — they
come back through M11 as configured servers instead, which is the whole argument for M11.

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
| `init_tracing` | `mod.rs` 30 | The code is trivial; the doc comment is the point — under stdio, stdout **is** the JSON-RPC channel and one stray log line corrupts framing. Mooshik meets this from both sides once M11 spawns servers. |

**Grep by name, not by line.** Those numbers are true at `166a3c8` and will not stay true —
`server.rs` went from 2,577 to 4,625 lines in a day while J1 and J2 landed.

**Depends on:** M2, M3.

---

## M5 — Permissions

The `[permissions]` block from spec §3.6, enforced in Rust **at the tool-call boundary**, plus a
command that prints the resolved grant set.

This is the design, not a feature of it. Autonomy is the sum of grants; without enforcement the
companion loop is a chat client with a memory attached. And the enforcement point must be one
place, because a check duplicated per tool is a check that will be forgotten by the fourth tool.

**The graph is never a permission authority.** No concept, however canonical, widens a grant. A
memory that says "you may run scripts" is a string, not a capability — and since the bootstrap
ingests documents written by other people, this is a real injection path, not a theoretical one.

**Depends on:** M4.

---

## M6 — The vault

`~/.mooshik/vault`, encrypted, 0600, never synced, never embedded. A `secret set` / `get` / `list`
CLI. Values injected into *tools* at use time — process env for scripts, headers set inside
Mooshik for HTTP — never into a prompt, transcript or `lambo_derive` call.

**Egress redaction is the part that actually earns the design.** The leak path is tool *output*: a
script that echoes `$TOKEN`. Every tool result is scanned against vault values before it reaches
the model or the graph. Everything else about the vault prevents secrets from entering; this is
the one place a secret has already left and must be caught.

**Open decision:** keyring or passphrase. OS keyring is friendlier and machine-bound; an Argon2id
passphrase is portable and survives a misbehaving keyring. This decides whether an ambient,
always-on Mooshik can start unattended — which, for a companion whose whole premise is being
always on, is a product decision rather than a security one.

**Depends on:** M1.

---

## M7 — The CLI

**Not a phase. A rule: nothing is done until it has a CLI surface.**

M7 sits here in the numbering for the decision it carries, but the work is spread across M2–M6.
Every milestone lands with the commands that drive it — `mooshik chat`, `recall`, `stats`,
`secret set/get/list`, `permissions`, `config` — written as that milestone is written, not
retrofitted afterwards.

The reason is scheduling, not taste. A surface built at the end is a surface built under deadline
pressure, and it is the part a judge, a reader and a future contributor actually touch. Built
alongside, it costs each milestone an hour. Built at the end, it costs a day nobody has.

**Decided: a chat with context.** A persistent conversational loop, not one-shot subcommands.
Ambience and continuity are the product's claim, and a one-shot command cannot demonstrate either.
The other commands stay one-shot; `chat` is the one that holds a session.

This decision does more work than it looks like it does — see below.

**Depends on:** M3 for `chat`; each other command on its own milestone.
**Done when:** every shipped capability is reachable from the CLI, with `--help` that reads like it
was written on purpose, and the demo can be driven end to end without the TUI existing.

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

So M2 gains a task: **Mooshik binds a session endpoint and publishes it like a holder does.**

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

The flush interval is a product setting, not a performance knob. Pick it deliberately in M1.

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

**Open decision: where does it live?** It is Python, and Mooshik is Rust. Its own repo is cleaner
to reason about; a subdirectory here means one link in the submission and one clone for a judge.
Judges have to find it either way, so the tiebreaker is probably the submission, not the code.

Ingest rules, unchanged from the hackathon doc: allowlist by extension rather than denylist, a
secret scanner over every candidate document with a hit dropping the document rather than
redacting it, and **no `git diff` content** — commit messages and metadata only, because diffs are
where secrets hide.

**Depends on:** M2, and Lambo's A and B.

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

---

## M11 — The MCP host

*(An id, not a position. This lands after M6 and before M10.)*

Without this the tool surface is seven tools welded into the binary, and every new capability is a
Rust PR. With it, capability is configuration: a search server restores `search_web` and
`fetch_page` without either being written here, and `[mcp_servers.*]` in `config.toml` covers
github, obsidian and whatever else without touching the tool code.

`rmcp` is already a Lambo dependency for its MCP **server**; this is the **client** half of the
same crate.

Do not expect to lift code from `src/mcp/`. It is the other half of the protocol —
`#[tool_router]`, `ServerHandler`, inbound HTTP transport, rate limiter, auth token — and none of
it is a client. What M4 lifts from there is listed under M4; M11's own plumbing is new. The one
exception worth reading first is `proxy.rs`, which is a client: it dials a socket and forwards
tool calls, which is structurally what M11 does to every configured server.

**Copy Lambo's dependency line carefully.** It pins `default-features = false` and its comment says
that is required rather than stylistic: rmcp's optional `reqwest` is `^0.13.2`, and a repo pinning
reqwest 0.12 compiles it twice otherwise. Mooshik pins reqwest for the companion client, so it
walks into the same trap. Features here are `client` and `transport-child-process`.

### Two constraints, or this breaks things already decided

**Exposure is an allowlist, per server.** Aggregation's default behaviour is to hand the model
every tool every server offers, and spec §3.1 trims the surface precisely to stop a small local
model misrouting. Config names which tools are exposed; the companion still sees roughly eight,
not forty. Without this, M11 breaks the local-companion premise rather than extending it.

**Server credentials come from the vault.** Spec §3.4's example puts `GITHUB_PERSONAL_ACCESS_TOKEN
= "ghp_..."` inline in `config.toml`, which contradicts M6 outright. Env is resolved from the vault
at spawn time. This is also the better vault demonstration — a real credential reaching a real tool
and never a prompt, rather than a script echoing a variable.

MCP tools are tools: they pass M5's gate like everything else, which means the grant syntax needs
to name them (`mcp.github.*`). That is the one place M5 has to be written knowing M11 exists.

**Depends on:** M4, M5, M6.

**Cost, honestly:** process lifecycle, stdio JSON-RPC, discovery, and reconnect-on-crash is a real
day even with rmcp, and it competes with M8 and M9. If the clock forces a choice, this is the
better thing to cut than M9 — M9 is the finding, this is plumbing that will still be here in
September.

---

## M10 — The TUI

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

---

## Decisions needed, in the order they bite

| # | Decision | Blocks | Cost of deciding late |
| --- | --- | --- | --- |
| ~~1~~ | ~~One session or many~~ | — | **Decided: one unified session.** See M2. |
| ~~2~~ | ~~`run_scratch_script` in scope~~ | — | **Decided: in scope.** See M4. |
| 3 | Vault key: keyring or passphrase | M6 | Decides whether Mooshik can start unattended |
| ~~4~~ | ~~Chat loop or one-shot CLI~~ | — | **Decided: a chat with context.** See M7. |
| 5 | Ingester repo location | M8 | Cheap now, awkward once CI and docs exist |
| 6 | Default grant set for the demo | M5 | Only bites at recording time, but bites hard |
| ~~7~~ | ~~Where the surface effort goes~~ | — | **Decided: CLI throughout, TUI last.** See M7 and M10. |
| ~~8~~ | ~~Companion loop: framework or hand-rolled~~ | — | **Decided: hand-rolled.** See below. |

Five settled. The two remaining are genuinely late-binding: the vault key source when M6 starts,
the ingester's home when M8 does. Decision 6 only bites at recording time.

**On decision 8**, since it will be proposed again: the companion targets exactly one wire format,
OpenAI-compatible `/v1`, which every candidate endpoint already speaks. A framework's value is an
abstraction over provider variation Mooshik does not have, and the parts of M3 that are actually
hard — partial-stream cancellation, a tool call arriving mid-stream, context pressure answered by
recall rather than truncation — are not what a framework hands you. The deciding argument is M5:
the permission gate belongs at the tool-call boundary, and owning the loop is how that is held
outright instead of depending on someone else's hook granularity. The dependency taken is an SSE
parser, not a harness.
