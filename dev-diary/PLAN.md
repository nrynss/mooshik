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
M0 ─→ M1 ─→ M2 ─→ M4 ─→ M5 ─┐
      └───→ M6 ──────↗  │    ├─→ M10
            M3 ─────────┘    │
M2 ─→ M8 ─→ M9 ──────────────┘

M7 is not a node. The CLI grows with M2–M6; see M7.
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

Out, per the hackathon doc: `search_web`, `fetch_page`, `delegate_to_coder`, and MCP host
aggregation. The companion can talk and remember; it does not browse.

Delegation stays out on scope, not on difficulty. A coding agent is a subprocess with a prompt and
a working directory, so when it lands it is one tool behind the existing gate and sandbox — no
bridge, no sidecar. Nothing in M4 needs to anticipate it.

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

### 1. Two writers on one session, and the store now refuses

Lambo's single-writer lease became **store-enforced** in T8.6: a per-session lease row acquired
atomically, *"refused fail-closed when a live one is already held by someone else"*, with
explicitly no preemption — a live holder keeps its heartbeat and the newcomer simply loses.

Combine the two decisions already made — one unified session, plus an always-on Mooshik holding
`lambo::Memory` in process — and the bootstrap ingester writing through `lambo serve` is a
**second writer on the same session**. It does not queue, retry or degrade. It is refused.

The cheapest resolution for this month: **the bootstrap is a one-time job and Mooshik is stopped
while it runs.** Write that down as an operational fact rather than discovering it as a failed
Cloud Run job on day 4. The alternatives are worse: routing Mooshik through `lambo serve` too
contradicts the in-process design in spec §2, and giving the bootstrap its own session contradicts
the one-session decision.

Worth stating in the write-up rather than hiding — an always-on companion and a bulk importer
genuinely contend for one memory, and fail-closed is the right answer.

### 2. A crash loses the write-behind tail

The lease module is explicit that a lease expiring proves nothing about durability: *"the tail
lived in the crashed process's in-RAM log and died with it."* A long-running chat process
accumulates unflushed mutations between flush intervals, so a crash silently loses the most recent
memory — the part the user just created and is most likely to notice missing.

The flush interval is now a product setting, not a performance knob. Pick it deliberately in M1.

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
volume, and writes through `lambo serve` over MCP as the single writer.

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

**Depends on:** M8.

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
