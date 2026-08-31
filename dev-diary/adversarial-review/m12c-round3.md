# M12c round 3 — adversarial re-verification of the round-2 remediation

Reviewed against HEAD `e63c23b` (`110781a fix(vertex)` + `e63c23b
test(ingester)` are the OPERATOR's ingester commits, above the M12b close
`e7365e8`; not M12c, left untouched), branch `main`, tree dirty with the M12c
implementation + both remediations (no commits). Scope: all four findings in
`m12c-round2.md` (F1-F4), the two new pins the remediation added, the
milestone's two load-bearing behaviors end-to-end, and the M12b/M12a
regression pins. All runs in a clean env (`env -u LAMBO_POSTGRES_DSN -u
MOOSHIK_POSTGRES_DSN -u DATABASE_URL`). The lambo crate is pinned at git rev
`4c6fc93` (Cargo.lock confirms). Every mutation below was transient and
reverted: the touched file was restored from a byte copy taken before the
mutation and `md5sum`-verified identical afterwards. `git status --porcelain`
at the end shows the same M12c set as at the start (10 modified + 5
untracked), plus this record. Nothing committed by this round.

## Verdict

**APPROVE.** All four round-2 findings are genuinely fixed and every pin
bites: the F1 reroute leg is real (deleting the reroute loop now fails the
pin at the 3-way in-count assert, where round 2's same mutation was green),
the F2 gutter produces the spec's four-word lines with an honest empty day
(both stub-restore mutations fail both pins; the real binary prints
`gutter 2026-08-31: the ring twelve copies`), the F3 dispatch arm is pinned
(deleting the arm fails `reflect_dry_run_through_dispatch_reports_without_writing`,
where round 2's same mutation was green), and the F4 decoy is gone (no
`_unused_anchor_for_type_check`, no `HashMap`/`SessionId` import-keep-alive;
clippy clean). Both load-bearing behaviors hold end-to-end through the real
binary. The one documented limit — the returns key ties 2:2 under the
in-count-3 fixture, so a returns-only comparator reversal leaves the (d) leg
green — is real but not material: the strongest-survives property is defended
at the comparator level (full-reversal mutation fails (d)) and at the
timestamp tie-break that actually decides in this fixture (latest-only
reversal fails (d)). Zero findings, zero residue. **M12c is clean and ready to
commit.**

## What held

- **F1 (P2) — the reroute leg bites.** The fixture now has a third
  interaction `i3` whose `Derives` edge targets the loser — a reroutable edge
  distinct from the loser's origin (`i1`) and the survivor's sources (`i1`,
  `i2`); timestamps are ordered so `strongest_first`'s latest-event tie-break
  keeps the survivor on top once both concepts carry two returns. The pin
  asserts the survivor's in-count is exactly the set
  `{origin, second_turn, third_turn}` (3, id-sorted both sides — robust to
  lambo's neighbor ordering) and the loser's incoming is exactly `[origin]`.
  Executed mutations: reroute loop deleted → **fails at 328:9 (b)**
  (`left: 2, right: 3`, the 3-way message); marker drops `]: {original}`
  (snapshot + live) → **fails at 310:9 (a)**; full comparator reversal →
  **fails at 281:5 (d)**; `is_already_merged` false → **fails at 295:5 (e)**.
  The remediation's honest caveat is confirmed by execution: a returns-only
  reversal leaves (d) green — the returns key ties 2:2 by the fixture's
  design — but a latest-only reversal fails (d), so the leg that decides in
  this fixture is pinned, and the wholesale-order regression the (d) leg
  names is caught. Judged an acceptable documented limit, not a weakening.
- **F2 (P2) — the gutter is real.** `FixtureReflector::day_gutter` sorts the
  day's entities by length, keeps the top four, and maps each through
  `four_word_summary` (no `#[allow(dead_code)]`; the comment states the real
  call path). A day with nothing to summarize yields an honest empty gutter.
  Executed: stub restored → **both pins fail**
  (`day_gutter_is_four_word_lines_…` at prose.rs:479:9 assert_ne! and
  `day_gutter_stays_empty_…` at prose.rs:503:9). Binary proof on a fresh
  seeded sqlite home (fixture embedder, two turns reaching the entity "the
  ring holds five hundred and twelve copies"; transient
  `tests/seed_gutter_r3.rs`, deleted after): `mooshik reflect --dry-run`
  prints `gutter 2026-08-31: the ring twelve copies` — the four-word line the
  spec names, no false `Nothing on record`.
- **F3 (P3) — the dispatch arm is pinned.** `reflect_dry_run_through_dispatch_reports_without_writing`
  now reads `mod.rs` via `include_str!` and asserts
  `dispatch.contains("memory_cmd::reflect(&layout, args)")`, following the
  `chat_command_never_opens_memory` convention exactly (tests.rs:105-108
  pins `chat_cmd::chat(&layout)` the same way); the doc no longer overstates
  the claim. Executed: reflect arm deleted from `mod.rs` → **the pin fails at
  tests.rs:869:5** — where round 2's run of the same mutation was green.
- **F4 (P3) — the decoy is gone.** `src/memory/reflect/mod.rs` imports
  `HashSet` (used by `collect_days`/`collect_thread_anchors`) and
  `Interaction` (used by `write_prose_concept`); no `HashMap`, no `SessionId`,
  no `_unused_anchor_for_type_check` (grep across the repo finds it only in
  the round-1/round-2 records). Clippy clean without any keep-alive.
- **The milestone holds end-to-end.** All four milestone pins green on the
  clean tree: `the_view_surfaces_the_prose_reflect_wrote`,
  `re_running_reflect_is_first_write_only`, `an_unreflected_graph_keeps_the_empty_prose_defaults`,
  `reflect_help_comes_from_text_and_parses_the_dry_run_flag`. Through the
  real binary on the seeded home: `reflect --dry-run` reports the four prose
  rows and `Dry run: nothing written.` (exit 0); `reflect` writes them
  (`Reflect pass written.`, exit 0); a second `reflect` prints
  `nothing to write` — first-write-only holds through the CLI.
- **M12b/M12a regression pins all green** on the clean tree, individually:
  `the_figures_are_read_before_the_graph_guard` and
  `the_build_runs_against_the_copy_and_not_the_guard` (the guard-duration syn
  pins — the prose index is built inside `of_graph` after the guard drops),
  `the_scratch_sandbox_and_script_stay_private`,
  `two_sandboxes_opened_in_the_same_instant_are_two_directories`,
  `a_termination_signal_disposition_is_restored_after_the_session`,
  `the_local_database_is_created_and_repaired_private`.
- **Operator isolation.** `git diff e7365e8 e63c23b` touches only
  `ingester/`, `src/companion/google_tests.rs`, `src/config/{companion.rs,
  mod.rs}` and one line of `src/cli/tests.rs`
  (`vertex_base_url("mooshik", "us-central1")` → `"global"` in
  `the_google_posture_is_reachable_from_the_cli_alone` — a vertex test, not
  M12c). The M12c additions in `tests.rs` (the reflect block) are intact in
  the working-tree diff against HEAD; no M12c file's M12c content was
  disturbed. `e63c23b` touches only `ingester/tests/test_ingest.py`.

## Findings

None. Zero findings, zero residue within the documented limits.

## Mutation table

All runs in the clean env; `reflect_tests` pins on the live-sqlite store
pattern, CLI pins through the real clap tree. Every mutation reverted and
`md5sum`-verified byte-identical after its run.

| # | Mutation | Pin | Result |
|---|---|---|---|
| 1 | Reroute/upsert loop deleted (`apply_cluster`, paraphrase.rs) | `consolidation_write_path_…` | **fails at 328:9 `(b)` — left 2, right 3, the 3-way in-count message. Bites (was green in round 2)** |
| 2 | Marker drops `]: {original}` (both snapshot and live writes) | `consolidation_write_path_…` | **fails at 310:9 `(a)` — the loser's original content is gone from the marker. Bites** |
| 3 | `strongest_first` comparator reversed (weakest first, all five keys) | `consolidation_write_path_…` | **fails at 281:5 `(d)` — the weakest concept would survive. Bites** |
| 4 | `is_already_merged` returns `false` | `consolidation_write_path_…` | **fails at 295:5 `(e)` — re-plan after the apply is not empty. Bites** |
| 5 | Returns comparison reversed only | `consolidation_write_path_…` | **passes — the documented limit: returns tie 2:2 by the fixture's design; the (d) leg is defended at the comparator level (m3) and at the deciding timestamp tie-break (m6). Accepted, not a finding** |
| 6 | Latest-event comparison reversed only | `consolidation_write_path_…` | **fails at 281:5 `(d)` — the timestamp tie-break is pinned. Bites** |
| 7 | Gutter stub restored (`day_gutter` returns `["Nothing on record"]`) | `day_gutter_is_four_word_lines_…`; `day_gutter_stays_empty_…` | **both fail — prose.rs:479:9 assert_ne! and prose.rs:503:9 assert_eq!. Bites** |
| 8 | Reflect dispatch arm deleted (`mod.rs`) | `reflect_dry_run_through_dispatch_reports_without_writing` | **fails at tests.rs:869:5 — the arm pin. Bites (was green in round 2)** |

## Gates (run by me, clean env)

- **`cargo test --locked`** — **568 passed, 0 failed, 2 ignored** (lib; the 2
  ignored are the pre-existing live-Cloud `memory::ops::tests`) **+ 1 passed**
  (integration `tests/report_pin.rs`, 30.01 s), exit 0. Ran on the clean tree
  before any mutation; the post-mutation tree was re-verified byte-identical
  (md5) and the F1/F2/F3 pins re-run green on it.
- **`cargo clippy --all-targets --all-features`** — exit 0, zero warnings.
- **`cargo fmt --check`** — exit 0.
- **File-size caps** — `view.rs` 992/1000 (under cap; the milestone has no
  further `view.rs` work planned). Reflect files: mod.rs 409, prose.rs 518,
  paraphrase.rs 490, snapshot.rs 197, reflect_tests.rs 385; CLI files mod.rs
  85, tests.rs 935, memory_cmd.rs 69, render.rs 142 — all well under.

## Executed vs read

**Executed:** the eight mutations above (each reverted and md5-verified), the
twelve pins on the clean tree and after each mutation (F1 pin ×6 runs, F2
pins, F3 pin, the four milestone pins, the six M12b/M12a regression pins), the
full suite, clippy, fmt, and the real binary end-to-end — seeded home via a
transient `tests/seed_gutter_r3.rs` (deleted after; temp home under gitignored
`target/`, removed): `reflect --dry-run`, `reflect`, `reflect` again. The
binary output above is the record's evidence for the F2 gutter behavior, the
write, and the first-write-only re-run on the real surface.

**Read (not re-executed):** lambo `4c6fc93` sources — the (b) set assertion
sorts both sides by `.0`, so `in_neighbors_typed`'s ordering does not affect
it, and (c) is a single-element assert, order-independent; the M12b round-8
record for the regression-pin names and the record shape.

## Notes for M12d

- The round-2 "unattributed ingester changes" note is resolved: the operator
  committed them (`110781a` + `e63c23b`) during the round-2 remediation;
  nothing pending there.
- F1's returns-tie is structural to the in-count-3 fixture (documented above):
  the (d) leg is defended at the comparator level and at the timestamp
  tie-break, both mutation-verified. A future fixture with a returns
  differential (e.g. a third source on the survivor) would make the
  returns-only reversal visible too; not needed today.
- `view.rs` sits at 992/1000. M12d (the watcher) should not add to it without
  extracting a module.
- `run_reflect` swallows `record_cluster_action`'s error (`let _ =`, mod.rs:227):
  the audit row is best-effort — a fenced session silently loses the merge's
  provenance row while the merge persists. Deliberate; the marker still makes
  the merge reversible. Carried from round 2, still not a finding.
- When a live `CompanionReflector` lands, the gutter seam should get the
  documented four-word behavior (F2's fix) — the fixture must not be the only
  shape ever exercised.
- `of_graph` walks the concept list once more per tick for the prose index
  (`ProseIndex::from_concepts`); M12b's measured ~29 ms release budget at the
  4k shape should be re-measured with prose present before M12d leans on it.
