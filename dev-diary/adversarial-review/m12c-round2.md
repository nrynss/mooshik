# M12c round 2 — adversarial re-verification of the round-1 remediation

Reviewed against HEAD `e7365e8`, branch `main`, tree dirty with the M12c
implementation + the round-1 remediation (no commits). Scope: all eight
findings in `m12c-round1.md`, the write-path fix the remediation added
(`write_prose_concept` writing the Concept directly with its `mooshik-prose:`
key instead of the `derive_async_as` path), and the milestone's two
load-bearing behaviors end-to-end. All runs in a clean env (`env -u
LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`). The lambo crate
is pinned at git rev `4c6fc93` (Cargo.lock confirms). Every mutation below was
transient and reverted: `src/memory/reflect/` restored from a pre-session byte
copy and `diff -r`-verified identical after each run; the tracked files
(`src/cli/mod.rs`, `src/cli/command.rs`, `src/memory/view.rs`) restored and
their `git diff HEAD` reviewed line-for-line against the original patch.
`git status --porcelain` at the end shows the same M12c set as at the start
(10 modified + 3 untracked), plus this record — and four **unattributed**
working-tree changes in `ingester/` (README.md, agent.py, config.py,
extraction.py: a gemini-2.5-flash → gemini-3.7-flash bump and a new
`INGEST_LOCATION` env override, 16 insertions / 5 deletions, mtime 10:09:36
during this round's mutation window). Those edits are not mine and no peer
claims them; I left them untouched — confirm ownership before committing.
Nothing committed by this round.

## Verdict

**REMEDIATE** — the two round-1 P1s are genuinely fixed and demonstrable
end-to-end (both verified through the real binary below), but not every pin
bites: the R5 consolidation pin's reroute/absorption legs are **vacuous**
(deleting the whole reroute loop leaves the pin green — F1), the R1-1 CLI pin
does not cover the dispatch arm (deleting the arm leaves every CLI test green —
F3), the default reflector's gutter is a stub that contradicts the spec and its
own docs (F2), and the remediation's "unused imports dropped" claim is kept
alive by a decoy function (F4). 2×P2 + 2×P3. The milestone's core behaviors
work today; the residue is pin coverage, one wrong default output, and one
import-keep-alive.

## What held

- **R1-1 (P1) — the CLI is real.** `src/cli/command.rs` registers `reflect`
  with a `--dry-run` `ArgAction::SetTrue` flag, `dispatch` routes
  `Some(("reflect", args))` to `memory_cmd::reflect`, the handler opens via
  `resolve::load_with_secrets`, runs `run_reflect(..., &FixtureReflector,
  dry_run, now)`, closes, and prints `render::render_reflect`. The `[reflect]`
  section exists in `src/text/en.toml`. **Verified through the real binary on
  a live seeded sqlite home** (`MOOSHIK_HOME=target/m12c-cli-home`, fixture
  embedder, two turns derived): `mooshik reflect --help` prints the subcommand
  help and the `--dry-run` flag (exit 0); `mooshik reflect --dry-run` prints
  `Reflect pass:` with the four planned prose rows and `Dry run: nothing
  written.` (exit 0); `mooshik reflect` prints the same rows then `Reflect pass
  written.`; a second `mooshik reflect` prints `nothing to write` — the
  reopen-after-write reads the `mooshik-prose:` keys back from disk, so
  first-write-only holds through the CLI too. The dry-run's no-write half is
  additionally pinned by `reflect_dry_run_through_dispatch_reports_without_writing`
  (reopen finds `ProseIndex` empty) and the parse/help shape by
  `reflect_help_comes_from_text_and_parses_the_dry_run_flag`.
- **R1-2 (P1) — the view surfaces reflect's prose, and the write path is
  fixed.** `of_graph` builds one `ProseIndex::from_concepts(&graph.concepts)`
  and threads it into `days` (`mood`/`highlights`/`notes` via `prose_for_day`)
  and `threads` (`because` via `reason_for_thread`); the empty defaults hold
  (`day_prose` default → `mood: None`, empty gutter/notes;
  `unwrap_or_default()` → `Justification::default()`), pinned by
  `an_unreflected_graph_keeps_the_empty_prose_defaults`. `write_prose_concept`
  now writes the Concept directly on its own interaction with the structured
  `mooshik-prose:<field>:<target>` canonical key (`ProseConcept::as_concept`),
  bypassing `derive`. The round-1 failure mode is reproduced and caught:
  mutating the written key to be derived from the prose *text* (the old
  `derive_async_as` behavior) fails **both** `the_view_surfaces_the_prose_reflect_wrote`
  and `re_running_reflect_is_first_write_only` — a reopened session's
  `ProseIndex::from_snapshot` finds nothing by prefix, so the pane shows no
  mood and the second run re-plans prose. The lambo `4c6fc93` sources confirm
  the fix is sound at the store level: `insert_concept` writes the
  `canonical_key` as given (no text derivation) and Observation key collisions
  are exempt from the uniqueness check, so the explicit key round-trips to
  disk and back.
- **R3/R6/R8** — `cargo fmt --check` exit 0; the unused `session` binding is
  gone from `run_reflect`; `anyhow = "1"` / `argon2 = "0.5"` are back at
  column 0 (uuid added as a direct dependency with a comment).
- **R7 — first-write-only, judged coherent and documented honestly.**
  `prose.rs` module doc, `mod.rs` Storage section and the `[reflect]`
  `after_help` all state it plainly ("a day or thread keeps the prose its
  first reflect run wrote, and a re-run skips it"); `plan_reflect` asks the
  `ProseIndex` first. Against the plan (PLAN.md M12c: "writes the prose M12a
  deliberately leaves empty"), first-write-only is the coherent reading of the
  in-body "one prose concept per day / per thread" invariant — refresh-on-rerun
  would duplicate or overwrite the canonical-key space without the spec asking
  for it, and the canonical key is what keeps the "one concept" invariant true.
  The re-run path is pinned (`re_running_reflect_is_first_write_only`) and was
  demonstrated through the CLI (second `mooshik reflect` → `nothing to write`).
  Not a finding.
- **R4 — clippy clean** (`--all-targets --all-features`, exit 0, zero
  warnings) — except the two smells below, one of which is the
  `four_word_summary` allow the remediation claims is justified. It is not:
  the allow masks a function nothing in production calls (F2), and the
  `_unused_anchor_for_type_check` decoy keeps `HashMap`/`SessionId` imports
  alive (F4).

## Findings

| # | Pri | File | Finding |
|---|---|---|---|
| F1 | P2 | `src/memory/reflect/reflect_tests.rs:288-306` (fixture 196-244) | **The R5 consolidation pin's reroute/absorption legs are vacuous — deleting the entire reroute loop still passes.** Executed: removing the "Re-upsert the rerouted edges" block from `apply_cluster` (paraphrase.rs:321-338) leaves `consolidation_write_path_preserves_losers_absorbs_derives_and_is_a_no_op` **green**. The fixture's loser is reached only by its own origin interaction (i1); its sole incoming `Derives` edge *is* the structural one, so (c) "the loser's only incoming edge is its own origin" holds trivially and (b) "survivor absorbs the loser's derives" is asserted at `len() == 2` — exactly the survivor's pre-merge count (i1 + i2). A regression that stops rerouting — the milestone's "strongest survives with full history" property: every turn that reached the loser must reach the survivor — ships silently. **Fix:** give the loser a reroutable edge from a third interaction distinct from its origin and from the survivor's sources (timestamps ordered so the survivor stays strictly strongest), then assert the survivor's in-count is 3 and the loser's is exactly `[origin]`. |
| F2 | P2 | `src/memory/reflect/prose.rs:278-287` (344-370) | **The default reflector's gutter is a stub, and `four_word_summary` is genuinely dead.** `FixtureReflector::day_gutter` computes and discards the sorted entity list (`sorted` is never read) and always returns `["Nothing on record"]` — `lines` is empty at the check by construction. It never calls `four_word_summary`, whose `#[allow(dead_code)]` justification ("`day_gutter` feeds its result through `dyn Reflector`") is false; the only callers are its own unit tests. Impact is on the real surface: a seeded binary run of `mooshik reflect` on a day with two turns and an entity printed `gutter 2026-08-31: Nothing on record` — a false statement on the pane — contradicting the module doc ("the gutter summary is the four strongest concepts in four-word windows") and the milestone spec (PLAN.md: "its four-words-a-line gutter summary"). **Fix:** implement `day_gutter` from the top-4 entities via `four_word_summary` (or delete the dead fn and correct the docs). |
| F3 | P3 | `src/cli/tests.rs:897-901` (doc 852-858) | **The R1-1 "dispatch" pin does not cover `dispatch`.** The test calls `super::memory_cmd::reflect(&layout, &sub)` directly, bypassing the `Some(("reflect", args))` arm; deleting that arm from `mod.rs` leaves every CLI test **green** — `mooshik reflect` would silently become a no-op (`_ => Ok(())`, exit 0) with no test failure, while the test's doc claims it runs "through the dispatch path". The codebase has an established arm-pin convention (`chat_command_never_opens_memory` reads `mod.rs` via `include_str!` and asserts `chat_cmd::chat(&layout)`); reflect's arm has none. (The routing works today — binary-verified — the gap is protection plus the overstated claim.) **Fix:** extend the `include_str!("mod.rs")` source-read pin to the reflect arm, or drive `dispatch`/`run` with a `reflect` argv. |
| F4 | P3 | `src/memory/reflect/mod.rs:388-389` | **`_unused_anchor_for_type_check` is an import-keep-alive decoy.** A `#[allow(dead_code)]` function whose only purpose is to keep the `HashMap`/`SessionId` imports on line 54/57 "used" — neither name appears anywhere else in `mod.rs`. The remediation claims the unused imports were dropped; in `mod.rs` they were preserved via a fake function. **Fix:** drop the imports and the function. |

## Mutation table

All runs in the clean env; `reflect_tests` pins on the live-sqlite store
pattern, CLI pins through the real clap tree.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | `Some(("reflect", args)) => …` dispatch arm deleted (`mod.rs`) | `reflect_dry_run_through_dispatch_reports_without_writing` | **passes — gap (F3): the test calls the handler directly, so the arm is unprotected** |
| 2 | `--dry-run` flag dropped (`command.rs`) | `reflect_help_comes_from_text_and_parses_the_dry_run_flag` | **fails at 822:5 (`get_long() == Some("dry-run")`) — bites** |
| 3 | Prose canonical key derived from the *text* (old `derive_async_as` failure mode, `prose.rs as_concept`) | `the_view_surfaces_the_prose_reflect_wrote`; `re_running_reflect_is_first_write_only` | **both fail — bites: reopen finds nothing by prefix; round-1 failure mode reproduced** |
| 4 | Reroute/upsert loop deleted (`apply_cluster`) | `consolidation_write_path_…` | **passes — vacuous legs (F1): no reroutable edge exists in the fixture** |
| 5 | `strongest_first` returns-comparison reversed (weakest first) | `consolidation_write_path_…` | **fails at 254:5 `(d) the strongest concept must survive` — bites** |
| 6 | Marker drops `]: {original}` (both snapshot and live writes) | `consolidation_write_path_…` | **fails at 283:9 `(a) the loser's original content must survive verbatim` — bites** |
| 7 | `is_already_merged` returns false | `consolidation_write_path_…` | **fails at 268:5 — bites: (e) applied-then-replan empty is pinned** |
| 8 | `of_graph` given `ProseIndex::default()` (`view.rs`) | `the_view_surfaces_the_prose_reflect_wrote` | **fails at 75:5 (`today.mood` None) — bites: the R1-2 seam is pinned** |

## Gates (run by me, clean env)

- **`cargo test --locked`** — **565 passed, 0 failed, 2 ignored** (lib; the 2
  ignored are the pre-existing live-Cloud `memory::ops::tests`) **+ 1 passed**
  (integration `tests/report_pin.rs`, 30.02 s), exit 0. The M12b `syn`-based
  guard-duration pins in `view_session_tests.rs` still hold with the prose
  index added to `of_graph` (the index is built inside `of_graph` on the copy,
  after the guard drops).
- **`cargo clippy --all-targets --all-features`** — exit 0, zero warnings.
- **`cargo fmt --check`** — exit 0.
- **File-size caps** — `view.rs` 992/1000 (under cap; the milestone has no
  further `view.rs` work planned, so the 8-line headroom is not blocking —
  but M12d must not grow it further without extracting). Reflect files:
  mod.rs 412, prose.rs 429, paraphrase.rs 490, snapshot.rs 197,
  reflect_tests.rs 348; CLI files memory_cmd.rs 69, render.rs 142 — all well
  under.

## Executed vs read

**Executed:** the eight mutations above (each reverted and verified); the six
pins on the clean tree and after each mutation; the full suite; clippy; fmt;
and the real binary end-to-end — `init`, `reflect --help`, `reflect
--dry-run`, `reflect`, `reflect` again, on a fresh provisioned sqlite home
seeded with two turns (transient `tests/seed_cli_turn.rs`, deleted after;
temp home under gitignored `target/`, removed). The binary output is the
record's evidence for the CLI rows, the write, the first-write-only re-run and
the F2 gutter behavior.

**Read (not re-executed):** lambo `4c6fc93` sources (`insert_concept` writes
the given canonical key; `upsert_edge` reinforces on duplicate natural key
`(source, target, edge_type)`; `remove_node` clears all incident edges via
adjacency) — the write-path claims were checked against these rather than
re-deriving them; PLAN.md M12c spec for the first-write-only judgment and the
gutter intent; the m12b round-8 record for the guard-pin scope and the record
shape.

## Notes for M12d

- The ingester working-tree changes (gemini-3.7-flash + `INGEST_LOCATION`) are
  unattributed; confirm ownership before the commit that follows an APPROVE.
- `view.rs` sits at 992/1000. M12d (the watcher) should not add to it without
  extracting a module.
- `of_graph` now walks the concept list once more per tick for the prose index
  (`ProseIndex::from_concepts`); M12b's measured ~29 ms release budget at the
  4k shape should be re-measured with prose present before M12d leans on it.
- When a live `CompanionReflector` lands, the gutter seam should get the
  documented four-word behavior (F2's fix) — the fixture must not be the
  only shape ever exercised.
- `run_reflect` swallows `record_cluster_action`'s error (`let _ =`, mod.rs:227):
  the audit row is best-effort — a fenced session silently loses the merge's
  provenance row while the merge persists. Deliberate; the marker still makes
  the merge reversible. Noted, not a finding.
