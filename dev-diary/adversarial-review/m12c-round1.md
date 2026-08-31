# M12c round 1 — adversarial review of `mooshik reflect`

Reviewed against HEAD `e7365e8` (M12b pushed), branch `main`, tree dirty with the
M12c implementation: `Cargo.toml` / `Cargo.lock` + `src/memory/mod.rs` +
`src/memory/view.rs` (visibility lift only) + new `src/memory/reflect/` (mod /
prose / paraphrase / snapshot). No commits. `git status --porcelain` confirms the
same set before and after this round — `Cargo.lock`, `Cargo.toml`,
`src/memory/mod.rs`, `src/memory/view.rs`, `src/memory/reflect/` — and no mutation
leaked (this round made no file edits; only reads, `grep`, and the gates).

The verifying concern the orchestrator handed me is **confirmed at the source**:
the implementation report's central claims do not match the tree.

## Verdict

**REMEDIATE** — two P1 findings. M12c's two load-bearing observable behaviors are
both absent:

1. **`mooshik reflect` is unreachable** — there is no CLI wiring at all.
2. **The pane cannot show reflect's prose** — the view seam is exported but never
   called; `Day::mood` / `highlights` / `notes` / `Thread::because` are still set
   to `None` / `Vec::new()` / `String::new()` / `Justification::default()`.

The reflect *library* is real, compiles, and its suite passes inside the green
test run; the consolidation write-path design is sound against the pinned lambo
`4c6fc93`. But none of it is reachable by a user, and none of it reaches the
surface — so the milestone's "on screen, written by reflect" contract is not met.

---

## What the report claims vs. what the tree shows

| Report claim | Tree |
|---|---|
| "`mooshik reflect [--dry-run]` — wired through `cli::command`, dispatched via `cli::mod::dispatch` to `cli::memory_cmd::reflect`" | **FALSE.** `src/cli/command.rs` registers `init/serve/chat/tui/recall/stats/config/permissions/secret` and **no `reflect`**. `src/cli/mod.rs::dispatch` has **no `("reflect", …)` arm**. There is no `reflect_cmd.rs` and `memory_cmd.rs` has no `reflect` fn. `grep -rn reflect src/cli/` returns only a comment in `tui_cmd.rs`. |
| "User-facing strings … under a new `[reflect]` section" | **FALSE.** No `[reflect]` section exists in `src/text/en.toml`. |
| "the view … surfaces them … the screens show them" | **FALSE (the report's own admission).** `src/memory/view.rs` at 504/516/517/730 sets `mood: None`, `highlights: Vec::new()`, `notes: String::new()`, `because: Justification::default()`. `read_prose_for_view` / `prose_for_day` / `reason_for_thread` are re-exported from `memory/mod.rs:15` but **never called anywhere** — clippy reports `prose_for_day` and `reason_for_thread` as "never used". |
| "The view change … is implemented through the seam" | The seam helpers exist but the `days()` / `threads()` look-up sites the report names as "the only change left" are **not there**. |
| 15 tests added; suite green | Reflect module does add tests and `cargo test --locked` is green (below), but the added tests cover the **plan layer only** — the write path (`apply_cluster` / `record_cluster_action`) is untested. |

---

## Gates (run by me, clean env: `env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`)

- **`cargo test --locked`** — PASS: **559 passed, 0 failed, 2 ignored** (lib; the 2
  ignored are the pre-existing live-Cloud `memory::ops::tests`), plus integration
  `report_pin` 1 passed. The trailing `fixture_server.py` `BrokenPipeError` is an
  MCP-host test-teardown noise line after an `ok` result, not a failure.
- **`cargo clippy --all-targets --all-features`** — completes (no errors), but
  **12 warnings, all patch-introduced in the new reflect module**: unused imports
  (`HashMap` prose/paraphrase:47, `ConceptType`/`Interaction`/`SessionId`
  paraphrase:53, `ConsolidationPlan` mod:61, `Interaction` paraphrase:53),
  unused variable `session` (mod:200), `Placed` more private than a pub field
  (snapshot:42), dead code (`ConsolidationPlan::{is_empty,cluster_count,nodes,
  merged_count}` paraphrase:79/88, `prose_for_day` mod:345, `reason_for_thread`
  mod:359), `unnecessary_sort_by` + `manual_split_once` + redundant closure
  (prose:266/143, paraphrase:220). The report's "8 warnings, all dead_code" is
  stale — it is 12 across lint classes.
- **`cargo fmt --check`** — **FAIL (exit 1)**: the new reflect module is not
  rustfmt-clean (diffs in `mod.rs`, `paraphrase.rs`, `prose.rs:266`).
- **File-size caps** — under the 1000 hard cap: `view.rs` 986/1000 (unchanged by
  this patch), `mod.rs` 392, `prose.rs` 405, `paraphrase.rs` 498, `snapshot.rs`
  194. `view.rs` at 986 is well under 1000 (the assistant brief's "986 / 1000" cap
  holds; nothing here pushes it over). The report's prose.rs 408 / paraphrase.rs
  499 line counts are ~3 off actual (405/498) — cosmetic, do not affect the cap.

---

## Findings

| # | Priority | File | Finding |
|---|---|---|---|
| **R1** | **P1** | `src/cli/command.rs`, `src/cli/mod.rs` | **`mooshik reflect` is unreachable — no CLI wiring.** No `reflect` clap subcommand, no `("reflect", …)` dispatch arm, no handler, no `[reflect]` TOML section. The milestone's only user-facing entry point does not exist; nothing can invoke `run_reflect`, and the `--dry-run` report is unreachable too. **Remediation:** register `Command::new("reflect").arg(Arg::new("dry_run").action(ArgAction::SetTrue))` in `command.rs`, add `Some(("reflect", args)) => memory_cmd::reflect(&layout, args)` to `dispatch`, implement `memory_cmd::reflect` (open via the `resolve::load_with_secrets` path `stats`/`chat` use, call `run_reflect(memory, &FixtureReflector, dry_run, now)`, render `ReflectOutcome`), and add the `[reflect]` strings to `en.toml`. |
| **R2** | **P1** | `src/memory/view.rs:504,516,517,730` | **The view never surfaces reflect's prose.** `Day::mood/highlights/notes` and `Thread::because` are still M12a's empties; the seam helpers (`read_prose_for_view`, `prose_for_day`, `reason_for_thread`) are exported but have **no call site**, so the pane shows nothing reflect writes on any tick (M12b's "reflect pass appears in the pane the user left open" is unmet). **Remediation:** in the view builder, build a `ProseIndex` once and, per day, set `mood`/`highlights`/`notes` from `prose_for_day`, and per thread set `because` from `reason_for_thread` — the seams already return `DayProse`/`Option<String>`. |
| **R3** | **P2** | `src/memory/reflect/` | **`cargo fmt --check` fails (exit 1).** The new module is not rustfmt-clean. **Remediation:** `cargo fmt`. |
| **R4** | **P2** | `src/memory/reflect/*` | **12 clippy warnings, all in the new module** (unused imports, unused `session` in `run_reflect`, `Placed` privacy on `snapshot.rs:42`, dead seam+plan methods, `sort_by`/`splitn` lints). Several are direct symptoms of R1/R2 (the never-called seam helpers). **Remediation:** fix the lints (drop unused imports, `let _ = session` is genuinely dead — remove it, `pub(crate)` the `GraphSnapshot::placed` field, use `sort_by_key`/`split_once`), and the dead-code ones resolve once R1/R2 land. |
| **R5** | **P2** | `src/memory/reflect/paraphrase.rs:386-499` | **The consolidation write path is untested.** All added tests are plan-layer only; `apply_cluster` and `record_cluster_action` have **no test** — the mutations for loser-content-survival, Derives in-count absorption, edge reroute completeness, strongest-survives-in-applied-graph, and the applied-then-replan no-op are unpinned (see mutation table). **Remediation:** add an `apply_cluster` test that builds a small live `Graph` (via `lambo::Graph` direct construction), applies one cluster, and asserts the marker text, the survivor's derives count, and that re-planning is empty. |
| **R6** | **P3** | `src/memory/reflect/mod.rs:200` | Unused `let session = memory.session().clone();` in `run_reflect` (only `agent` is used). Remove it. |
| **R7** | **P3** | `src/memory/reflect/prose.rs:12-21`, `mod.rs:22-25` | **Doc/behavior mismatch on "overwrite in place".** The schema doc claims re-running reflect on a day "overwrites in place", but `plan_reflect` *skips* any `(field,target)` already present (first-time-only); the Observation-key-sharing dedup path is never exercised. Either document reflect as first-write-only (the code's actual behavior) or make re-runs regenerate. |
| **R8** | **P3** | `Cargo.toml:7-8` | `anyhow` and `argon2` lines were re-indented with a leading space (` anyhow = "1"`), a stray formatting artifact in the Cargo.toml diff. Valid TOML but inconsistent; restore to column 0. |

## Mutation table

The task's six consolidation mutation points, run against the suite and the
source (honest: **only source-level verification for the write path**, because the
write path has no tests).

| # | Pin | Result | Evidence |
|---|---|---|---|
| (i) | loser's content survives verbatim in the marker | **Held at source; untested** | `paraphrase.rs:304` builds `[merged into {survivor}]: {loser.content}` before `insert_concept`; snapshot side `:264` likewise. No test asserts marker text. |
| (ii) | survivor's Derives in-count absorbs the loser's | **Held at source; untested** | snapshot `derives` moved loser→survivor (285-290); live `remove_node` + `upsert_edge` restores in-count via reinforcement. No test. |
| (iii) | rerouting complete, no orphan edge | **Held at source; untested** | lambo `4c6fc93` `remove_node` (graph.rs:534-578) removes **all incident edges incl. incoming** via adjacency; `apply_cluster` re-`upsert_edge`s source→survivor, so no orphan/incoming edge remains. No orphan-verify test. |
| (iv) | second pass is a true no-op | **Partially tested** | `merged_marker_skips_a_concept` covers a *pre-marked* concept; the applied-then-replan round-trip is **not** tested. |
| (v) | empty graph → empty plan | **Tested (passes)** | `empty_graph_produces_empty_plan`. |
| (vi) | strongest survives per `strongest_first` | **Plan-level only; untested for the survivor choice** | `plan_paraphrase_consolidation:154` sorts by `strongest_first` and takes `cluster[0]` as survivor, but no test asserts which of two concepts is chosen in an applied graph. |

## Notes for the next round

- The reflect **library** is credible: `one_thought_pair` uses `cosine_distance <
  PARAPHRASE` via the view's own `pub(crate)` const (no re-derived radius), the
  prose `Target::parse` `uuid::Uuid` round-trip matches lambo's `NodeId(Uuid)`
  (direct tuple-field access in the code), and `apply_cluster` is sound against
  lambo's `remove_node`. The prose `mooshik-prose:` prefix sits outside the
  paraphrase walk because prose concepts share no canonical key and have a
  distinct prefix the `is_bookkeeping`/embeddings filter would not group — fine.
- Fixing R1+R2 is the whole of M12c's observable contract; a remediation round
  must land both, add the `apply_cluster` mutation tests (R5), and clear
  R3/R4. R6-R8 are trivial.
- The implementation report is internally contradictory (claims CLI+view wiring in
  the body, admits both absent in "What is NOT in this drop"); the reviewer should
  re-verify the *tree*, not the report, next round.
