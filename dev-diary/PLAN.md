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

M11 ─→ M12           the ambient layer: the data behind the surface

The CLI itself is not deferred to M7 — it grows with each milestone. M7 is the sweep.
M11 is last on purpose and is allowed to fail; see M11. M12 exists because it did not:
the surface landed, so the data behind it became worth building.
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
| **M11** | Built 2026-08-27, `m11-tui`. The surface is built; the data behind it is not — Today, the week and an 80x24 narrow layout draw from one view model, the `1i` palette rules are enforced in code and held by cross-screen tests, and `--demo` opens no database. Still allowed to fail: no capability lives only here. See M11 below. |
| **M12a** | Built 2026-08-30, `cf3dcbb`. `memory::view` reads the open graph into the view model: the week ending today, each day's log, the ribbon, what keeps coming back and what was just remembered, every placement resolved through `Interaction::about_time`. `mooshik tui` now holds the session for the length of the pane and closes it on the way out. Prose is deliberately unwritten — mood, gutter summaries, notes and a thread's reason are M12c's. Review rounds 2–6: **APPROVE**, zero residue (`m12a-round6.md`) — the graph's `-wal`/`-shm` claimed private, the scratch sandbox pinned 0700/0600 under a deterministic pin, signals restored after the session. See M12 below. |
| **M12b** | Built 2026-08-31. The tick: the redraw loop rebuilds the view model every 250 ms, so a write from the ingester, an MCP client or the reflect pass appears in the open pane without a keystroke. R1-3's deferred guard-duration item landed: the graph is copied out from under one short guard and the build runs against the copy, pinned by a structural (`syn`) guard-duration pin and measured (release embedded ~29 ms at the 4k shape vs the 250 ms budget). Review rounds 1–8: **APPROVE**, zero residue within the documented limits (`m12b-round8.md`). |
| **M12c** | Built 2026-08-31. `mooshik reflect [--dry-run]`: a one-shot consolidation pass that writes the prose M12a left empty — a day's mood, its four-words-a-line gutter summary, the trailing notes, and a thread's reason — as `mooshik-prose:` concepts the pane shows on the next tick, and merges the paraphrase twins into their strongest (loser content preserved, edges rerouted, audit row per cluster; re-runs are a true no-op). First-write-only by design. Review rounds 1–3: **APPROVE**, zero residue (`m12c-round3.md`). |
| **M12d** | Built 2026-08-31. The live pane owns a cancellable polling watcher for the current workspace: `.md/.markdown/.txt/.rst` files only, generated directories and symlinks excluded, file contents scanned with the ingester's whole-document secret policy but never derived. Git changes carry SHA/message metadata only, with commit author time; file events carry mtime. A 250 ms debounce coalesces bursts, and every derive uses the pane's shared `WriteLane`. The task is joined before `Memory::close`; it is never a daemon. Focused offline tests cover discovery/filtering, debounce replacement, secret drops, git metadata/time, and cancellation. |
| **M12e** | Built 2026-08-31, `49a504d` → `d5c5909`. `Enter` runs `Session::turn` on the pane runtime; tokens drain into the conversation; in-flight `Esc` cancels without quitting; a failed turn renders as a turn. Prompt-class tools denied on the pane path (stdin would hang). Execute-time diagnostics go through a sink, not `eprintln!`. Review rounds 1–2: **APPROVE**, zero residue (`m12e-round2.md`). See the M12 section. |
| **M12f** | Built 2026-08-31. `mcp-servers/artifacts/`: an MCP server that extracts typed concepts from screenshots and audio recordings using ADK `LlmAgent` + Gemini 3.7 Flash at `global`, returning them over stdio for Mooshik to derive in-process. Whole-document secret scanning (pattern + vault values) runs before concepts cross the wire. Uses `mooshik-common` for model defaults, Vertex client, and concept vocabulary. 14 offline tests. Review rounds 1–3: **APPROVE**, zero residue (`m12f-round3.md`). |
| **M12g** | Built 2026-08-31. `mcp-servers/coder/`: an MCP server that delegates code changes to external coding agents (Claude Code, Gemini CLI, Cursor Agent, Antigravity CLI) over stdio. Non-blocking fire-and-forget `delegate` returns immediately under the 60s host bound; `check` queries liveness; target repos refreshed with `AGENTS.md` standing rules from memory; ambient results captured by M12d watcher. `mooshik configure coder --agent <name>` writes config and vault secrets; grant set to `prompt`. 32 offline tests. |
| **M12h** | Not started. `mooshik init` prints three words and leaves a configuration that cannot work: `store` is postgres with no DSN, and the companion points at `127.0.0.1:8080` with model `local-model`. The guidance exists only as comments inside the file it just wrote. See the M12 section. |

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

**Status: the surface is built; the data behind it is not.** `mooshik tui` opens a `ratatui`
interface ported from a nine-artboard design (kept in `scratch_design/` while the port is in
progress). Six of the nine are ported — `1a`, `1b`, `1c`, `1d`, `1h` and the `1i` palette;
**`1e` (first run), `1f` (changing the database) and `1g` (its no-warning counterpart) are
deliberately out of scope**, because they are settings and lifecycle screens rather than the
companion surface this milestone is about, and every capability behind them is already reachable
from `mooshik configure` and the vault commands. What landed:

* **Today** (the default), **the week**, and an 80x24 narrow layout, all drawing from one plain-data
  view model, plus the two states the design makes the argument with — a thing the user said on
  another day quoted back inline, and one careful sentence before they contradict themselves. Both
  are turns in the same conversation rather than modals, which is the design's own point.
* The 16-colour palette with its rules enforced in code: no hex, no background but the ground, five
  bright slots deliberately unspent, red reserved to two uses, one double-ruled frame in the whole
  app. Cross-screen tests hold all of those at every terminal size.
* `--demo` draws the design's own day, reading no configuration and opening no database. It takes
  an optional scene — `--demo recall` for `1c`'s quoted words and `--demo caution` for `1d`'s one
  careful sentence — because both artboards are states of the conversation rather than screens of
  their own, and nothing else can reach them until the chat loop lands.

What is **not** wired: the status bar is live (from `memory::stats`), and the rest of the live
workspace is empty, because the data the artboards show has no source behind Mooshik yet — a day's
weather and mood have none at all, and per-day thread marks need recalled nodes grouped by event
date. Sending from the composer needs the M3 chat loop restructured into something a redraw loop
can drive; until that lands `Enter` is deliberately inert, `Alt-Enter` is unbound, and neither is
advertised on the composer's rule — a hint that does nothing is worse than no hint. The draft
stays where it was typed. The screens read the view model and never a store, so filling any of
this in is a change to `tui::live` and nothing else.

**Still allowed to fail.** No capability lives only here.

---

## M12 — The ambient layer

**The data behind M11's surface.** The TUI landed, which means the safety valve was never pulled and
the pane is now the face of the product — opening on empty panels, because the screens are a pure
function of a view model nothing was filling. M12 fills it, and then keeps it filled while the pane
is open. Four tasks, none of which touches a screen: the whole point of M11's shape is that this is
a change to what feeds the model.

* **M12a — the workspace snapshot.** Build `tui::model::Workspace` from the live graph: the seven
  days ending today and their logs, the ribbon, what keeps coming back, what was just remembered,
  and the clock. Every placement resolves through `Interaction::about_time` and a concept's is its
  origin turn's, so a bootstrapped decade does not collapse into the afternoon it was ingested. A
  pure function of a graph and an instant, testable without a terminal or a database.
* **M12b — the tick.** The redraw loop already wakes every 250ms so a resize is picked up. Rebuild
  the view model on that tick instead of once at startup, so a write from anywhere else — the
  ingester, an MCP client, the reflect pass — appears in the pane the user left open without a
  keystroke. The cost of a rebuild against a session-sized graph is the thing to measure first;
  M12a's builder is one pass over interactions and one over concepts, on purpose. The rebuild also
  closes R1-3's deferred guard-duration item: the whole pass used to run under one read guard, and
  at a 250ms cadence that would starve a writer. The graph is now copied out from under one short
  guard and the build runs against the copy — pinned by the guard-duration pins in
  `view_session_tests.rs` and measured with the copy in `view_tick_tests.rs`.
* **M12c — `mooshik reflect`.** A one-shot consolidation pass over the session. It writes the prose
  M12a deliberately leaves empty — how a day felt, its four-words-a-line gutter summary, the
  trailing notes on its detail pane, and why a thread sits where it does — and consolidates the
  paraphrase twins the post-M10 review measured. Everything on screen that is a written sentence
  rather than a fact comes from here, which is why M12a leaves the fields absent rather than filling
  them with a truncated log.
* **M12d — the watcher.** Filesystem and git changes under the workspace, derived as they happen, so
  the memory is ambient rather than something the user has to remember to tell. This is the task
  that makes the other three worth having. It runs on the pane's runtime and stops when the pane
  closes — see the daemon paragraph below. Its output reaches the screen through the graph and
  M12b's tick, never through the view model, which is why **M12d touches no file under `src/tui/`
  at all**: if it finds itself editing `app.rs` or the event loop, its design has gone wrong.

* **M12e — the pane converses.** M11 shipped a composer whose `Enter` does nothing and M12a–d
  filled in everything around it: the pane draws the week, refreshes itself, and writes its own
  prose. What it still cannot do is answer, and a companion surface that cannot be spoken to is a
  dashboard. `Enter` already maps to `Action::Send` (`input.rs:93`); the handler body is empty
  (`app.rs:182`) because sending used to clear the draft, which looked like sending and was silent
  data loss. The blocker was never the loop's logic. `run_chat` owns a tokio runtime, stdin and
  stdout, and the pane owns all three — two owners, not missing behaviour. One level down is the
  reusable unit: `Session::turn()` takes the user's text, a `Cancellation` and an `FnMut(&str)`
  token callback, and handles its own tool rounds. The pane holds a runtime for its lifetime,
  spawns a turn onto it, and passes a callback that sends into a channel the redraw loop drains.
  **Streaming is not an extra here.** `on_token` is a required parameter, so *not* streaming means
  passing a no-op closure — strictly more work for a worse pane. Cancellation is threaded through
  `turn()` and `client.complete` already, so `Esc` is the wiring the CLI gives Ctrl-C.

* **M12f — the non-text workspace.** `config.py:15` allows `.md,.markdown,.txt,.rst` and nothing
  else, so every screenshot, diagram, PDF and voice note in a workspace is invisible to the memory.
  That is not an incidental gap: **people capture precisely what is too tedious to type**, so the
  text record is lossy exactly where the images and recordings are. The standup note says windpipe
  was slow; the dashboard screenshot says p99 hit 4.2s at 14:20 and recovered at 14:47. The ADR
  written three days later has two branches; the whiteboard photo taken during the argument has
  four. Mooshik claims to remember your week and currently remembers the half you retyped. M12f
  closes that with an MCP server rather than a change to the ingester, so the capability is
  configuration and can be cut on the day without touching anything else.

* **M12g — the coding contractor.** The one milestone here that starts from a claim rather than a
  gap. `README.md:3`, the docs site's index and overview, and `text/en.toml:14` — which is what
  `mooshik --help` prints — all state in the present tense that Mooshik delegates heavy code edits
  to coding agents. `docs/SPEC.md` names the tool, `delegate_to_coder`, and is honest that it
  describes where the product is going; the other three dropped that qualifier. Nothing implements
  it: there is no `[coding_agent]` section, no delegation tool, and the shipped surface is the lambo
  tools plus `run_scratch_script` plus whatever MCP servers are configured. The repo is public and
  the contest requires the project to function as its text description says, so this is either built
  or the sentences are rewritten — and drifting into the deadline without choosing is the one option
  that is actually bad.

* **M12h — the guided first run.** Observed by installing v0.1.0 on a clean machine and typing
  `mooshik init`. It answers `Mooshik home initialized.` and stops. What it has actually written is
  a configuration in a state that cannot work: `[store]` is `postgres` with no DSN, and
  `[companion]` points at `http://127.0.0.1:8080/v1` with model `local-model`, which is a local
  posture nobody has running. Every instruction a user needs exists, as **comments inside the file
  init just wrote**, which means the product's setup path is "open the config and read it". The
  hackathon's default path is the Google one — Vertex for inference, Cloud SQL for the graph — and
  that is also Mooshik's shared posture, so it is the path `init` should walk somebody down, asking
  a question at a time and writing each answer as it goes.

**The daemon is explicitly out of scope.** A background process that outlives the terminal is a
different product decision — install, supervision, a second lease holder, and a thing running on a
machine when nobody asked it to. M12b's refresh happens inside the pane the user opened and stops
when they close it; M12c is a command they run; M12d watches for as long as something is open. If a
daemon is ever wanted, it is its own milestone with its own argument.

### M12d — what to watch, and what a burst costs

**What it calls.** `pane.spawner().spawn(...)` for the watch loop, and around every write:

```
let _lane = pane.writes().enter().await;
pane.memory().derive(...)
```

**Entering the lane is not optional.** Lambo does not serialize writers — see the concurrency note
under M12e. Two writers race, a lost race re-runs the whole gather *including a fresh embedder
round trip*, and past eight concurrent graph changes the derive fails outright. A watcher is the
one component that can produce writes in bursts, so it is the one most able to trigger that.

**Which makes debounce a correctness requirement, not a nicety.** A formatter on save, a `cargo
build` touching a tree, a branch checkout: all of them fire tens of events in a moment. Undebounced,
each becomes a derive, each derive costs an embedding call, and the pile races itself into the
replan cap. Coalesce a burst into one derive before it reaches the lane.

**Watch a workspace, not a filesystem.** The ingester walks an extension allowlist —
`.md,.markdown,.txt,.rst` — for good reason, and a watcher without one derives binary churn.
`target/`, `.git/`'s internals, `node_modules/` and the scratch sandbox must never be a source of
memory. Note the asymmetry: `.git/` internals are noise, but a *commit* is one of the strongest
signals available, carrying its SHA, repository, author time, and message.

**Decide what a change derives, because it settles the secret question.** Deriving file *content*
puts the watcher on the same footing as the ingester and it needs `secretscan.find_secret` with the
same whole-document drop — a watched file is exactly as able to hold a token as an ingested one, and
the pane path has no scanner today. Deriving *metadata* — that this file changed, that this commit
landed, with this message — is a much smaller exposure and probably the better first answer. Either
is defensible; shipping without choosing is not.

**Event time is the change's, not the clock's.** A file save is genuinely now. A commit is not:
carry its commit time, or a `git log` replayed on startup collapses history into this afternoon —
the pathology `backdate.py` exists to prevent, arriving by a different route.

**Depends on:** the seam (`ac4cfdc`), M2 for the graph, M12b for the tick that shows the result.
**Done when:** work done in an editor and a commit made in a terminal both appear in the open pane
with nothing typed into it.

**Implementation decision:** M12d takes the metadata-only option. A saved file is scanned in full,
including configured vault values while the vault lock is held, and a clean event derives only
`workspace file changed: <relative allowlisted path>` as an `Observation`; allowlisted files inside
repositories are deliberately excluded because repositories are metadata-only sources. A commit is scanned
for the same secret classes and derives `git commit <sha> in <repo>: <message>` as an `Observation`;
the git command is `--no-patch` and requests no parent, stat, or patch fields. This keeps the
ambient signal useful for recall without putting workspace prose or credentials into the graph.
The watcher polls the process's current working directory, because M12d has no separate workspace
setting, and stops with the pane; an existing repository's history is baselined rather than
replayed on open. Different historical event times are kept in separate derive groups so every
file mtime and commit author time remains truthful, even when one debounce window contains several
sources.
Successful derive batches are removed immediately; a failed batch stays pending for retry. As with
other ambient writes, a backend failure after a remote commit can therefore have at-least-once
semantics for that batch.
Discovery treats an empty repository with no `HEAD` as healthy and continues watching unrelated files;
Git command output is capped at 2 MiB, a poll admits at most 256 commits, and the pending queue holds
at most 2,048 events. Hitting a cap retains only the affected repository's old head and retries with
explicit backpressure; unrelated file state advances, and history is never silently dropped. Recursive
file reads use descriptor-relative `openat` traversal with
`O_NOFOLLOW` on Unix; the live watcher is disabled on platforms without an equivalent race-safe
descriptor/reparse-point primitive. A failed Git discovery — including a repository that appears
after the first poll — retains an explicit unknown-head state and replays reachable history after
recovery; only a genuinely new healthy repository is baselined. The live watcher is Unix-only and
fails closed at TUI startup (`WatchError::WorkspaceUnavailable`); the pane does not run without it.
Commit messages are byte-preserving for valid UTF-8, including
embedded NUL and record-separator bytes; invalid UTF-8 is replaced with U+FFFD at the graph boundary
so a valid commit can advance the repository head.

### M12e — what to move, and what not to break

**The three structural problems are solved. The seam (`ac4cfdc`) did them, so this milestone
calls rather than builds.** They are recorded here because the reasoning still governs what M12e
may do — the lease was claimed twice (`executor_for_chat` opened its own `Memory` while `tui_cmd`
held the lease); `run_chat` owned a runtime and `block_on`ed the session, which would take the
250 ms tick with it; and `run_chat_async` owns stdin and stdout, which the pane owns.

What M12e calls, all from `Pane` in `cli::tui_cmd` (private to that module — the milestone is
driven from `live()`, and a crate-visible `Pane` would be a second way to name the lease):

```
let ChatStack { tools, notices } = pane.tools(&config, vault, confirm);
pane.spawner().spawn(/* Session::turn */);
let _lane = pane.writes().enter().await;   // around anything that derives
```

`spawner()` hands back a `Handle`, not a `&Runtime`, so nothing spawned can outlive the pane.
`notices` are the assembly-time messages that used to be `eprintln!`s — render them, do not print
them.

**One line of plumbing is still M12e's own:** `compose_session` (`companion/chat.rs`) is a private
`fn` and has to become `pub(crate)`. The seam deliberately left it — adding visibility with no
consumer is speculative surface.

**Nothing on the turn path may print — and there are far more than two.** The first draft of this
section named the two `eprintln!` notices in `executor_for_chat`; building the seam found about
fifteen, and they split into two shapes that need different fixes. *Assembly-time* notices are
returned values — `executor_over_memory` hands back a `ChatStack { tools, notices }` and prints
nothing. *Execute-time* prints happen during a tool call and cannot be returned to anyone:
`tools/permissions.rs:102`, `tools/mod.rs:492/500/519`, and eight in `mcp_host/mod.rs` (284, 291,
295, 313, 369, 387, 399, 411). Those need a channel to the redraw loop, not a return value, and
every one of them corrupts the frame under the alternate screen. M12e owns that; the seam did not.

**There is a second stdin prompt, below the gate.** The spec named M5's gate only.
`tools/scratch.rs`'s `interactive_confirm` reads a line from stdin directly, and a pane built over
`MemoryTools::over` alone would hang on it even with a non-stdin gate. It is held shut by
`ScratchConfig::always_confirmed()`, which is why the seam made `chat_scratch` a shared constructor
rather than letting the pane path fall through to `ScratchConfig::default()`.

**A prompting tool must not reach for stdin.** The default grants allow the lambo memory tools
outright and *prompt* for scratch. A gate reading stdin while ratatui owns the terminal hangs the
pane with no way out. Either deny the prompt class deterministically on this path, or make approval
a turn in the conversation — which is what `1d` already establishes as the pattern, and the better
answer if it fits.

**A failed turn becomes a turn.** A 404, an expired token, a dropped stream: classified through
`cli::failure` and rendered in the conversation. Not a panic, and not silence.

**The panic contract survives the spawned task.** `tui_cmd` leaves the lease to lapse on its TTL
when the pane panics, because a handle dropped without a clean close is the crash-shaped path. A
turn task holding `Memory` must not outlive the pane, or it keeps writing after the terminal is
restored.

**The conversation becomes owned state.** `memory::view` sets `conversation: Conversation::default()`
deliberately — it is the chat path's, not the graph's. `App::refresh` already `mem::take`s it across
every rebuild, so a partial turn survives the tick with no extra care. That half is done.

**In-flight state has to be visible.** On `Enter` the draft moves into a sent turn and a pending
marker appears. Without it the key looks inert again, which is the precise failure the empty handler
was written to avoid.

**Concurrent writes are safe but not serial, and they can fail.** Established while building the
seam, against lambo `94cbf52`: writers take the **read** side of the writers gate
(`memory.rs:1292`, `begin_write` at 2873), because the write side belongs to `Memory::close` —
excluding a close is that lock's whole job, so N writers hold it at once. Commit is optimistic:
plan under a brief read lock, gather (embedding call included) with **no lock across the await**,
then commit only if the epoch has not moved. A lost race re-runs the whole gather including a fresh
embedder round trip, and past `MAX_HYBRID_REPLANS = 8` (`graph/hybrid.rs:196`) the call *fails* —
`"hybrid derive could not commit after 8 concurrent graph changes"`. Every `derive` opens an
interaction and bumps the epoch before it plans, so two writers genuinely invalidate each other.
This is why the seam carries a `WriteLane`: M12d's watcher and M12e's tool calls would otherwise
race each other into replans and, at eight, into a user-visible failure.

**Repaint granularity is the tick.** The loop blocks in `event::poll(TICK)`, so a token arriving
mid-poll does not wake it and the text grows in 250 ms steps. That reads as typing. Shorten the poll
while a turn is in flight if it should feel closer to a real conversation, and restore it when the
turn closes.

**Depends on:** the seam (`ac4cfdc`), M12a for the view model, M12b for the tick, M3 for the
session and its cancellation.
**Done when:** a question typed into the pane streams its answer back into the pane, `Esc` stops it,
and the memory it touches appears in the panels on the next tick. **Not required:** anything the
`1e`/`1f`/`1g` artboards cover — those stay out of scope for the same reason M11 gave.

**Status: built 2026-08-31.** Implement `49a504d`, remediate `d5c5909`, review rounds
1–2 **APPROVE** zero residue (`m12e-round2.md`). Prompt-class tools are denied on this
path rather than becoming a `1d` caution — PLAN allowed either. The write lane is held
inside `run_derive`, not around the whole turn (that would deadlock the single-permit
mutex).

### M12f — what is read, and what is refused

**The output is the same five typed concepts, never a caption.** `extraction.py:33` fixes the
vocabulary at `entity | logic | constraint | resource | observation`, and an artifact yields those
exactly as a markdown file does. The moment it emits "a screenshot of a dashboard with a rising blue
line" the graph has gained something nothing will ever recall.

Three targets, in priority order:

1. **Facts carrying values** — numbers, thresholds, timestamps, versions, error strings. `observation`
   and `constraint`. Highest value, because this is the part the text loses.
2. **Structure and relations** — a whiteboard box-and-arrow sketch is literally a graph, and turning
   it into `entity` nodes with `parent_of` edges feeds the retrieval mode that distinguishes Lambo:
   what rests on the thing you asked about, not what resembles it.
3. **Identity anchors** — the service, dashboard and resource names visible in the frame.

The third one decides whether this works at all. **An artifact whose concepts share no entity with
the existing graph is a dead island**: nothing links to it, nothing recalls it, and it never reaches
"what keeps coming back". The prompt biases toward naming entities the corpus already names.

**Refused, explicitly:** descriptions of the artifact as an artifact; UI chrome — window titles,
button labels, browser tabs, menu bars; OCR dumps of everything visible; anything that is not a
claim about the workspace. M9's finding applies unchanged — precision over coverage, and a sparse
graph beats a polluted one.

**The secret scanner is bypassed on this path, and that is a hole, not a nicety.** `pipeline.py:91`
runs `find_secret(doc.text, …)` and drops the *whole document* on a hit. An artifact has no
`doc.text`, and screenshots are among the highest-risk secret carriers there are: a terminal capture
with a token in scrollback, a connection dialog holding a DSN. The server runs `find_secret` over the
**extracted concepts** before deriving and drops the whole artifact on a hit — the same
whole-document semantics text gets. Without it M12f is the hole in a wall M5 and M6 spent two
milestones building.

**Provenance is the text path's, unchanged.** `event_time` from the file's mtime and `parent_of`
pointing at a `document_resource` for the artifact, or every extracted fact collapses onto today and
breaks the week placement — the pathology `backdate.py` exists to prevent.

**Audio is in, and is the more interesting half.** Verified live 2026-08-31 against
`gemini-3.7-flash` at `global`: an 11.4s wav returned a valid typed concept at 343 prompt tokens,
roughly 32 tokens per second of audio. A half-hour recording is one call at ~58k tokens and needs no
chunker at all, where text goes through the 4000-char chunk budget. And the two media are lossy in
different directions: **images carry precision, audio carries decisions and their reasons.** The
"we chose block over drop, because dropping strands the retry queue" is said aloud in every
engineering argument and written down in almost none — and when it is written down, the *because* is
the half that goes missing. `Thread::because` is a field M12c currently fills by inference; audio
would fill it with what was actually said.

**This is where ADK earns its place.** `agent.py`'s header argues a Runner is the wrong fit for the
batch path, and it is right: that loop is a deterministic map over chunks with checkpointing
between calls, which a Runner only obscures. Deciding what in an artifact is worth remembering is
the opposite shape — variable steps, a transcribe-then-extract path, and a real decision about
whether anything durable happened at all. So M12f is the first place ADK is used because it fits
rather than because it is on a requirements list.

**Shape — and the server does NOT write.** A new `mcp-servers/artifacts/` taking the *server
shape* from `mcp-servers/news/` — `server.py`, `tools.py`, the error and redaction handling, the
stdio wire tests — which is complete and tested. **Do not copy its config module.** The Vertex
client construction, the model and location defaults, and the concept vocabulary all come from
`mooshik-common`, which exists because copying those between packages is exactly how the same two
defects reached both the ingester and the news server on 2026-08-31. Depend on it, declare the exact
pin, and give this component its own `ARTIFACTS_LOCATION` override — never
`MOOSHIK_GEMINI_LOCATION`, which belongs to the embedder.

The dependency has a deployment consequence worth knowing before it bites: Mooshik spawns a server
as a bare `python3 /abs/path/server.py`, with no shell and no virtualenv activation, so
`mooshik-common` must be installed in *that* interpreter or the server dies at startup with
`ModuleNotFoundError`. It is an exact pin on no index, so it is installed from the path, first. Inside it an ADK
`LlmAgent` that **extracts and returns** typed concepts; Mooshik derives them through the in-process
memory tools it already owns. This is the ingester's one pattern that must not be copied: the
ingester writes through `LamboMcpWriter` because it runs standalone with its own session and its own
`mooshik serve`. An MCP server called while the pane is open cannot do that — `cli::tui_cmd` holds
the single-writer lease for the length of the session, and a second writer on the same session is
refused with exit code 2 and Mooshik's own conflict sentence. `mcp-servers/news` already models the
right shape: it returns and never writes. The secret scan therefore runs **in the server, before the
concepts cross the wire**, so artifact bytes never need to reach Mooshik at all. Reached as a
configured server — `mcp.artifacts.*`, gated by M5 like every other tool, credentials resolved from
the vault at spawn.

**Out of scope, stated so it is not rediscovered in review:** third-party consent. The text corpus
is what the operator wrote; a meeting recording contains people who did not agree to be someone's
memory. `find_secret` matches patterns — PEM blocks, `AKIA…`, `ghp_…`, `xox…` — and will never catch
a colleague's name and a private fact about them. That is private by content rather than secret by
pattern, and M12f does not solve it. The milestone accepts artifacts the operator places in their own
workspace and goes no further.

**Depends on:** M10 for the host, M5 for the gate, M6 for the credentials, M8 for the writer bridge
and the extraction prompt. **Done when:** a screenshot and a voice note dropped in the workspace
become typed concepts on the same threads the text corpus already names, and a secret in either one
drops the whole artifact.

### M12g — why it is a server, and why it cannot block

**No Rust.** This is an MCP server, the third one, taking its shape from `mcp-servers/news/` and its
constants from `mooshik-common`. That is the whole argument M10 exists to make: a new capability is
configuration, not a Rust PR. Building a first-class `[coding_agent]` section instead would
re-implement the host inside the binary.

**It cannot be one blocking call.** `MCP_CALL_WAIT` is a hard 60 s per tool call
(`mcp_host/mod.rs:64`) and it is a `const` with no per-server override. A real code change exceeds
that routinely, so a `delegate` that waits for a diff is a design that times out by construction.

**So it fires and the watcher learns the outcome.** `delegate(task, repo)` spawns the agent and
returns immediately with what it started — agent, repo, task — comfortably inside the bound.
`check(handle)` reports liveness and exit status. The *result* arrives the ambient way: M12d's
watcher sees the edits land and derives them, so the pane fills with what the contractor did while
it is still working. Nothing has to marshal a diff back through a tool call.

The daemon boundary holds without special handling: MCP servers are children of the host, so a
spawned agent dies when the pane closes — which is also the behaviour you want, since an agent
editing a repository after the user closed Mooshik is exactly the thing M12 refused to build. Bound
the child at spawn rather than relying on cleanup; a killed parent never runs its own teardown.

**Constraints do not need injecting — that problem is already solved, twice.** Lambo ships an MCP
server, and its README records three clients driving it model-first: OMP, Claude Code, and the
Cursor Agent CLI. And `lambo/dev-diary/notes/video-shoot.md` records *what actually made an agent
consult memory unprompted*: a standing rule it reads (`AGENTS.md`), plus a working directory where
grep answers nothing. That note is explicit that the MCP server's own `initialize` instructions were
**not enough on their own inside a code repository**. So `delegate` refreshes the standing rule in
the target repo from the graph and ensures the agent's own MCP config points at Lambo. The
contractor then consults memory itself, by the mechanism already proven to work.

**Grant it `prompt`, never `allow`.** This runs a code-editing agent against a real repository. It
is the clearest case in the product for asking first, and a better demonstration of M5 than the
scratch runner is.

**Ergonomics are the point, not an extra.** The friction is not the plumbing, it is that a user must
hand-write an `[mcp_servers.coder]` block with the right command, args, tool allowlist and vault
names. `mooshik configure coder --agent claude|omp|cursor` writes the block from a known-good
template, stores the credential in the vault under a name it then references, sets the grant to
`prompt`, and validates by spawning the server once and listing its tools. `mooshik config show`
already validates, so the last step is reuse. That is the difference between "capability is
configuration" as an architecture claim and as something a stranger can do in thirty seconds.

**Depends on:** M10 for the host, M5 for the gate, M6 for the vault, M12d for the watcher that
carries the result back. **Done when:** `mooshik configure coder --agent claude` writes a working
block, a delegated change edits a real repository under a standing rule drawn from the graph, and
the edits appear in the open pane without anyone describing them. **Or:** the four sentences are
rewritten to say what M10 already makes true — that a coding agent connects as a configured server
like any other tool — and this milestone is closed as a decision rather than left as a claim.

### M12h — what a first run has to say

**The failure discipline is already right. The sequencing is not.** Exit codes are correct: a
missing subcommand, a missing DSN and a missing store all exit `2`, and `mooshik recall` says
plainly "No Postgres DSN is configured." Nothing is broken. What is missing is any statement of
*what to do first*, and there are five ordered steps a new user cannot guess.

**`init` asks.** It walks the user through the Google path a question at a time, writing each answer
as it goes, and it ends with a Mooshik that runs. The user should not have to read a config file,
and should not have to copy five commands out of a printed list either. This is the first thing
anybody touches, and it is the one place where the product either sets itself up or does not.

Two constraints shape it, and neither is a reason to avoid prompting:

* **Prompt only when stdin is a TTY.** The Dockerfile calls `init` unattended and CI will too, so a
  non-TTY run keeps exactly today's behaviour and writes the defaults. A `--non-interactive` flag
  forces that path explicitly for a scripted run on a real terminal.
* **Read secrets with echo off, straight into the vault.** A DSN or a credential path is never
  echoed, never printed back, and never passed as an argument, so it reaches neither the scrollback
  nor `ps`. This is the reason to prompt rather than to print commands: `mooshik secret set` with a
  value on the command line is the exposure, and a no-echo prompt removes it.

**Re-runnable and resumable.** A user will answer half of it, get distracted, and come back.
Re-running `init` asks only for what is still unset, and confirms rather than clobbers anything
already configured. It is the same code path as first contact, so it cannot rot.

**`config show` reports what is still missing.** For the user who skipped the prompts, scripted the
install, or came back a week later. It already resolves and prints, so saying what is unset costs no
new surface, and a separate `doctor` is one more thing to discover.

**Errors should name the durable fix first.** `recall` currently says "Set MOOSHIK_POSTGRES_DSN",
which is the environment escape hatch. The path that survives a reboot is
`mooshik secret set <name>` plus `mooshik config set store.dsn_secret <name>`, and the message
should lead with that and offer the variable second. As written it teaches the wrong habit at the
first point of contact.

**The Google path is the one to walk, and it has a trap in it.** Inference is
`companion.auth google`, `companion.google_project`, `companion.google_location = global`,
`companion.model = gemini-3.7-flash`, `companion.google_credentials`. Embedding is
`embedder.gemini_project` with `gemini_location` left at `us-central1`. Those two locations differ
and must differ: Vertex serves Gemini 3.x from `global` only, and `gemini-embedding-001` lives in
the region. A user who "tidies" them into agreement breaks one or the other, so first-run should
state it rather than leave it to be discovered as a 404.

**Delete the stale example while you are here.** The template at `config.toml:36` and the
`config set --help` text both offer `gemini-2.5-flash`, which is below the floor every component
moved to on 2026-08-31. It is the first model name a new user reads.

**Depends on:** M1 for the home and the config writer, M6 for the vault, and nothing else.
**Done when:** a new user runs the install one-liner, runs `mooshik init`, answers its questions,
and reaches a working `mooshik chat` against Vertex without ever opening `config.toml` or copying a
command out of a list. A non-TTY `init` still writes the defaults and exits as it does today, and
`mooshik config show` tells anyone who took that route what is still missing.

**Depends on:** M11 for the surface, M2 for the graph. **Done when:** `mooshik tui` opens on the
user's own week.

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
