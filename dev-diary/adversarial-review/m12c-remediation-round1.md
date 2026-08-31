# M12c round-1 remediation

Remediates all eight findings in `m12c-round1.md` — the two P1s that are the
whole of M12c's observable contract (the CLI wiring and the view surfacing),
both P2s (fmt, clippy), the write-path coverage, and the three P3s. No
deferrals. Base and destination: branch `main`, tree left dirty for the
orchestrator, nothing committed. All runs in a clean env (`env -u
LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`).

## R1 (P1) — `mooshik reflect [--dry-run]` is wired through the CLI

**What was wrong.** The milestone's only user-facing entry point did not exist:
`src/cli/command.rs` registered no `reflect` subcommand, `dispatch` had no
arm, and no handler or `[reflect]` TOML section existed. `run_reflect` was
unreachable and the `--dry-run` report with it.

**The fix.**
- `src/cli/command.rs` — registered the subcommand with a `--dry-run`
  `ArgAction::SetTrue` flag, after `stats`, strings from `text`.
- `src/cli/mod.rs` — `Some(("reflect", args)) => memory_cmd::reflect(&layout,
  args)` in `dispatch`.
- `src/cli/memory_cmd.rs` — `reflect(layout, args)`: opens the home and loads
  config through `resolve::load_with_secrets` (the `stats` path), reads the
  `dry_run` flag, calls `run_reflect(&memory, &FixtureReflector, dry_run,
  now)`, closes the memory, prints `render::render_reflect`.
- `src/cli/render.rs` — `render_reflect(outcome, dry_run)` renders the prose
  rows and the merge rows (or `nothing_to_write`), closing with the
  `done`/`dry_run_done` lines. `FixtureReflector` is the default; the live
  `CompanionReflector` remains a named seam the offline suite never reaches.
- `src/text/en.toml` — new `[reflect]` section (`help`, `dry_run_help`,
  `after_help`, `header`, `prose_row`, `merge_row`, `nothing_to_write`,
  `done`, `dry_run_done`).
- `src/memory/mod.rs` — `impl From<ReflectError> for MemoryError` so the
  handler's `ReflectError` maps onto the existing `MemoryError` path.

**The pins that bite** (both in `src/cli/tests.rs`):
- `reflect_help_comes_from_text_and_parses_the_dry_run_flag` — the command
  tree exposes `reflect`, its `--dry-run` argument, and both
  `["mooshik","reflect","--dry-run"]` and `["mooshik","reflect"]` parse with
  `dry_run` true/false.
- `reflect_dry_run_through_dispatch_reports_without_writing` — drives the
  exact handler `dispatch` routes `reflect` to, via the real clap tree
  (`run_config_set`'s pattern), against a provisioned live sqlite home with a
  seeded turn; the dry run returns `Ok` and a reopen of the same store finds
  the `ProseIndex` **empty** — nothing was written. This is the CLI half of
  "reports the proposed set without writing"; the render's content is proven
  by `render_reflect` on the non-empty first-run outcome in the reflect
  integration test.

## R2 (P1) — the view surfaces reflect's prose

**What was wrong.** `Day::mood/highlights/notes` and `Thread::because` were
still M12a's empties; the seam helpers were exported but had no call site, so
the pane could not show anything reflect wrote. The round-1 review also found
the **write path** could never feed the view: `write_prose_concept` routed the
prose through `derive_async_as`, which derives the canonical key from the
*text*, so the stored concept's key was never `mooshik-prose:<field>:<target>`
and `ProseIndex` (which reads by that prefix) would find nothing. That is
fixed here too — it is the load-bearing half of the same finding.

**The fix.**
- `src/memory/reflect/prose.rs` — `ProseIndex::from_concepts(&[Concept])` (the
  read side the view's tick needs; `from_snapshot` delegates to the one parse
  rule). `from_key` uses `split_once`.
- `src/memory/reflect/mod.rs` — `write_prose_concept` now writes the prose
  concept directly on the graph (its own interaction, the structured
  `mooshik-prose:<field>:<target>` canonical key via `ProseConcept::as_concept`)
  rather than through `derive`, so the concept is readable by the view; the
  unused `session` binding is gone; `prose_for_day` / `reason_for_thread` are
  re-exported for the view.
- `src/memory/view.rs` — `of_graph` builds one `ProseIndex` and threads it into
  `days` (sets `mood`/`highlights`/`notes`) and `threads` (sets `because`). The
  added lookup stays bounded — `view.rs` is 992 lines, under the cap.

**The pin** (`src/memory/reflect/reflect_tests.rs`):
- `the_view_surfaces_the_prose_reflect_wrote` — writes prose through the
  fixture reflector into a live sqlite graph, reopens, `of_memory`, and asserts
  `today.mood` is `Some`, `today.notes`/`today.highlights` non-empty, and
  `threads[0].because` non-empty. This demonstrates the pane shows reflect's
  prose on the next tick.
- `an_unreflected_graph_keeps_the_empty_prose_defaults` — an empty graph still
  yields the empty defaults (no regression).

## R3 (P2) — `cargo fmt`

`cargo fmt` applied; `cargo fmt --check` exits 0.

## R4 (P2) — 12 clippy warnings

Clean under `cargo clippy --all-targets --all-features` (exit 0). Resolved:
- unused `HashMap` (paraphrase:47), `ConceptType`/`Interaction`/`SessionId`
  (paraphrase:53) — dropped; `ConceptType`/`SessionId` moved into the test
  module that uses them.
- unused `ConsolidationPlan` re-export (mod:61) — dropped, and the
  never-referenced `ConsolidationPlan::{is_empty,cluster_count,nodes,
  merged_count}` methods removed (call sites now read `.clusters` directly).
- unused `session` binding (mod:200) — removed.
- `Placed` more-private-than-`pub` field (snapshot:42) — `pub(crate)`.
- dead `prose_for_day` / `reason_for_thread` — resolved by the R2 view wiring.
- `unnecessary_sort_by` (prose:266) — `sort_by_key(Reverse(..))`.
- `manual_split_once` (prose:143) — `split_once`.
- redundant closure (paraphrase:220) — `unwrap_or_else(Utc::now)`.
- `four_word_summary` lib-target dead code — reachable only through the
  `Reflector` trait object (`day_gutter` feeds it via `dyn Reflector`), which
  the lib-target pass does not follow; the offline suite calls it directly, so
  it carries a documented `#[allow(dead_code)]`.
- an incidental rustdoc empty-line after a detached doc comment in `view.rs`
  was closed.

## R5 (P2) — consolidation write-path coverage

**Fix** (`src/memory/reflect/reflect_tests.rs`):
`consolidation_write_path_preserves_losers_absorbs_derives_and_is_a_no_op`
builds a real sqlite graph with two paraphrase twins (identical embeddings
below `PARAPHRASE`, distinct keys), one returned by two turns and one by one,
and applies the single cluster via `apply_cluster` + `record_cluster_action`.
It pins all five mutation points off the reopened store:
- (a) the loser's original content survives verbatim in the `merged:<uuid>`
  marker text;
- (b) the survivor's `Derives` in-count absorbs the loser's (2 sources);
- (c) rerouting is complete — the loser's only incoming `Derives` edge is the
  structural one from its own origin interaction, never a reroutable edge;
- (d) the strongest survives (asserted survivor == cluster survivor, matching
  `strongest_first`);
- (e) re-planning after the apply is empty (true no-op).

(Deliberately, the assertion reads the survivor's `in_neighbors_typed` counts
and the loser's own-origin-only edge rather than "zero edges to the loser":
`apply_cluster` re-inserts the loser with `insert_concept`, which recreates the
structural `Derives` edge from its origin — that origin edge is the provenance
fact, and every *reroutable* edge has left.)

## R6 (P3) / R7 (P3) — first-write-only prose, documented and pinned

The module docs previously claimed re-running reflect "overwrites in place via
canonical-key sharing", but `plan_reflect` skips targets that already have
prose (first-write-only). Resolved honestly **as first-write-only** — the
code's actual behavior and the in-body spec note ("one prose concept per
day"): `prose.rs`/`mod.rs` storage docs now say a day or thread keeps the prose
its first reflect run wrote, and a re-run skips it. `reflect_tests.rs` pins it:
`re_running_reflect_is_first_write_only` runs `run_reflect` twice and asserts
the second run plans zero prose and the reopened index holds exactly the first
run's count — no duplicates, no overwrites.

## R8 (P3) — Cargo.toml indent

Restored `anyhow = "1"` and `argon2 = "0.5"` to column 0.

## Gates (clean env)

- `cargo test --locked` → **565 lib passed, 0 failed, 2 ignored** (the
  pre-existing live-Cloud/print-only two) **+ 1 integration passed**
  (`tests/report_pin.rs`, 30.02 s), exit 0.
- `cargo clippy --all-targets --all-features` → clean, exit 0.
- `cargo fmt --check` → clean, exit 0.
- File-size caps clean: `view.rs` 992 (under 1000), all reflect files and the
  CLI/`render` files well under.

## How each P1 is demonstrated working

- **P1 R1-1** — `reflect_dry_run_through_dispatch_reports_without_writing`
  drives the exact routed handler through the real clap tree against a live
  sqlite home: `--dry-run` returns `Ok` and a reopen shows no prose written;
  `reflect_help_comes_from_text_and_parses_the_dry_run_flag` shows `--help`
  exposes the subcommand and the `--dry-run` flag.
- **P1 R1-2** — `the_view_surfaces_the_prose_reflect_wrote` writes prose via
  the fixture reflector into a live sqlite graph and asserts `of_memory`'s
  `Day::mood/notes/highlights` and `Thread::because` carry it; the
  `re_running_reflect_is_first_write_only` pin guards the prose counts.
