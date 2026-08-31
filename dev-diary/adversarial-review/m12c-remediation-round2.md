# M12c round-2 remediation

Remediates all four findings in `m12c-round2.md` — the two P2s (the
consolidation pin's vacuous reroute/absorption legs, the default reflector's
stub gutter) and the two P3s (the unprotected reflect dispatch arm, the
import-keep-alive decoy). No deferrals. Base and destination: branch `main`,
tree left dirty for the orchestrator, nothing committed. All runs in a clean
env (`env -u LAMBO_POSTGRES_DSN -u MOOSHIK_POSTGRES_DSN -u DATABASE_URL`).
Every mutation below was transient and reverted: the touched file was restored
from a byte copy taken immediately before the mutation and `md5sum`-verified
identical afterwards; the four files whose bytes changed this round
(`src/memory/reflect/mod.rs`, `src/memory/reflect/prose.rs`,
`src/memory/reflect/reflect_tests.rs`, `src/cli/tests.rs`) were additionally
`diff`ed against their pre-round snapshots to confirm the change set is exactly
the four fixes below.
`git status --porcelain` at the end shows the same M12c set as at the start
(10 modified + 5 untracked), plus this record — with one difference: the
**four unattributed `ingester/` working-tree changes** the round-2 record
listed (gemini-2.5-flash → gemini-3.7-flash bump, `INGEST_LOCATION` override)
are gone from the dirty set because **the operator committed them during this
round**: `110781a fix(vertex): ask global for Gemini 3.x, in the ingester and
the companion` (10:22:59, ingester/README.md, ingester/ingester/{agent,config,
extraction}.py, src/cli/tests.rs ±1, src/companion/google_tests.rs,
src/config/companion.rs, src/config/mod.rs) and `e63c23b test(ingester): the
ADK surface is exercised, not just declared` (10:29:21). HEAD moved from
`e7365e8` to `e63c23b` mid-round. I never touched `ingester/` and made no
commits; the M12c implementation and all four fixes below remain uncommitted
in the working tree (verified: `git show HEAD:src/memory/reflect/paraphrase.rs`
fails — the whole reflect module is untracked; the F3 arm pin is absent from
HEAD's `tests.rs`, i.e. the operator staged before my edit landed). Nothing
committed by this round.

## F1 (P2) — the consolidation pin's reroute/absorption legs were vacuous

**What was wrong.** `consolidation_write_path_preserves_losers_absorbs_derives_and_is_a_no_op`
passed with the entire reroute loop deleted from `apply_cluster`: the fixture's
loser was reached only by its own origin interaction, so its sole incoming
`Derives` edge *was* the structural one, (c) held trivially, and (b) asserted
`len() == 2` — exactly the survivor's pre-merge count (origin + one return).
A regression that stops rerouting had nothing to bite on.

**The fix** (`src/memory/reflect/reflect_tests.rs`). The fixture now has a
third interaction `i3` whose `Derives` edge targets the **loser** — a real
reroutable edge, distinct from the loser's origin (`i1`) and from the
survivor's sources (`i1`, `i2`). Timestamps are ordered so the survivor stays
strictly strongest per `strongest_first`: `i1` at `base`, `i3` at
`base + 10s`, `i2` at `base + 20s` — once both concepts carry two returns,
`strongest_first`'s latest-event tie-break (`i2` later than `i3`) keeps the
survivor on top, so (d) still picks `survivor_id`. The assertions now read:
(b) the survivor's `Derives` in-count is **3** — origin + its own second turn
+ the third turn's rerouted edge — asserted as the exact set
`{origin, second_turn, third_turn}` in id order (lambo's `in_neighbors_typed`
returns id-ascending, verified in the pinned `4c6fc93` sources); (c) the
loser's incoming is exactly `[origin]`, unchanged.

**The pins that bite** (each mutation transient, reverted, `md5sum`-verified):

| # | Mutation | Result |
|---|---|---|
| 1 | Reroute/upsert loop deleted (`apply_cluster`, paraphrase.rs:321-338) | **fails at 328:9 `(b) the survivor absorbs the loser's derives — origin, its own second turn, and the rerouted third` — bites (was green pre-fix)** |
| 2 | Marker drops `]: {original}` (both snapshot and live writes) | **fails at 310:9 `(a) the loser's original content must survive verbatim in the marker` — bites** |
| 3 | `strongest_first` comparator reversed (weakest first: returns, days and latest keys all flipped) | **fails at 281:5 `(d) the strongest concept must survive` — bites** |
| 4 | `is_already_merged` returns `false` | **fails at 295:5 `(e) re-planning after the apply must be empty — the loser is marked merged` — bites** |

One nuance, stated honestly: with the survivor's in-count pinned at **3**
(origin + one non-origin + one rerouted), both concepts necessarily carry
exactly origin + one non-origin source, so the returns key is tied at 2:2 by
design — the survivor's strict strength sits on the latest-event tie-break
(that is what "timestamps ordered so the survivor stays strictly strongest"
means). A mutation that reverses *only* the returns comparison therefore leaves
(d) green; the regression (d) names — "the weakest concept survives" — is
defended at the comparator level (mutation 3 above). The round-2 mutation #5
("returns-comparison reversed") bit on the *old* fixture only because there the
returns differed (2 vs 1); under the in-count-3 fixture the returns tie is
structural. This is the price of the non-vacuous reroute leg, and it is the
fixture the assignment specifies.

## F2 (P2) — the default reflector's gutter was a stub

**What was wrong.** `FixtureReflector::day_gutter` sorted the day's entities
and then discarded them — `lines` was empty by construction, so every day with
turns printed `Nothing on record`. A real binary run on a day with two turns
and an entity showed exactly that false statement on the pane, contradicting
the module doc ("the four strongest concepts in four-word windows") and the
M12c spec. `four_word_summary` was genuinely dead: its `#[allow(dead_code)]`
justification ("`day_gutter` feeds its result through `dyn Reflector`") was
false — `day_gutter` never called it; the only callers were its own unit tests.

**The fix** (`src/memory/reflect/prose.rs`). `day_gutter` now takes the day's
entities, sorts by length descending, keeps the top four, and maps each through
`four_word_summary` — the four-words-a-line gutter the spec names. A day with
nothing to summarize yields an honest **empty** gutter (no false
`Nothing on record` claim). `four_word_summary` is now genuinely reachable:
`FixtureReflector::day_gutter` calls it and `plan_reflect` reaches the impl
through the `Reflector` trait object, so the `#[allow(dead_code)]` is gone and
its comment now states the real call path.

**The pins that bite** (`day_gutter_is_four_word_lines_from_the_top_four_entities`,
`day_gutter_stays_empty_on_a_day_with_no_entities`): a fixture day with five
entities of distinct lengths yields exactly the four-word lines of the four
longest (`["a very artboard layout", "the ring twelve copies",
"mum called mid-incident", "drinks off"]`, with the fifth-longest excluded),
and `assert_ne!` guards the false `["Nothing on record"]`; a day with a turn
but no entity yields an empty gutter. Mutation (stub restored): **both pins
fail** — `day_gutter_is_four_word_lines_...` at prose.rs:486:9 (left
`["Nothing on record"]`) and `day_gutter_stays_empty_...` at prose.rs:510:9.

**End-to-end, through the real binary** (mirroring round 2's repro): a fresh
`mooshik init` home, seeded with two turns reaching the entity "the ring holds
five hundred and twelve copies" (fixture embedder, transient
`tests/seed_gutter_turn.rs`, deleted after), then:

```
$ mooshik reflect --dry-run
Reflect pass:
  mood 2026-08-31: An ordinary day
  gutter 2026-08-31: the ring twelve copies
  notes 2026-08-31: You wrote 2 turns and noted 1 thing on this day.
  thread_reason fd3e3a1b-0fbc-4642-bdb6-06b5a46ecb28: You came back to the ring holds five hundred and twelve copies 2 times, across 1 day.
Dry run: nothing written.
```

`gutter 2026-08-31: the ring twelve copies` — the four-word line the spec
names, where round 2's run printed the false `Nothing on record`. (Temp home
under gitignored `target/`, removed.)

## F3 (P3) — the CLI pin bypassed `dispatch`

**What was wrong.** `reflect_dry_run_through_dispatch_reports_without_writing`
called `super::memory_cmd::reflect(&layout, &sub)` directly, bypassing the
`Some(("reflect", args))` arm; deleting that arm from `mod.rs` left every CLI
test green while `mooshik reflect` would silently fall through `_ => Ok(())`,
exit 0. The codebase already had the arm-pin convention
(`chat_command_never_opens_memory` reads `mod.rs` via `include_str!` and
asserts `chat_cmd::chat(&layout)`); reflect's arm had none.

**The fix** (`src/cli/tests.rs`). The dispatch pin is now inside
`reflect_dry_run_through_dispatch_reports_without_writing`, following the chat
convention exactly: `include_str!("mod.rs")` and
`assert!(dispatch.contains("memory_cmd::reflect(&layout, args)"))`, with the
test's doc comment stating the arm is pinned the same way `chat`'s is (no more
overstated "through the dispatch path" claim — it is now literally true).

**The pin that bites.** Mutation (the `Some(("reflect", args))` arm deleted
from `mod.rs`): the test **fails** at tests.rs:869:5 —

```
dispatch must route `reflect` to the pinned handler: //! The command-line surface: parse argv, dispatch, classify what went wrong.
```

— where round 2's run of the same mutation was **green**.

## F4 (P3) — `_unused_anchor_for_type_check` was an import-keep-alive decoy

**What was wrong.** `src/memory/reflect/mod.rs:388-389` kept the `HashMap` and
`SessionId` imports (lines 54/57) alive via a `#[allow(dead_code)]` function;
neither name was used anywhere else in the file, so the round-1 "unused imports
dropped" claim was false in `mod.rs`.

**The fix** (`src/memory/reflect/mod.rs`). Dropped `HashMap` from line 54
(`HashSet` stays — `collect_days` uses it) and `SessionId` from line 57
(`Interaction` stays — `write_prose_concept` constructs one), and deleted the
decoy function. Nothing else referenced it (grep across the repo). `cargo
clippy --all-targets --all-features` is clean without the decoy — the imports
that were "kept alive" are simply gone.

## Gates (clean env, run by me on the final tree)

- **`cargo test --locked`** — **568 passed, 0 failed, 2 ignored** (lib; the 2
  ignored are the pre-existing live-Cloud `memory::ops::tests`) **+ 1 passed**
  (integration `tests/report_pin.rs`, 30.02 s), exit 0. The two new gutter
  pins are the +2 over the pre-round reflect set (`src/memory/reflect` now has
  21 tests; `src/cli/tests` 33).
- **`cargo clippy --all-targets --all-features`** — exit 0, zero warnings
  (including with `four_word_summary`'s allow removed).
- **`cargo fmt --check`** — exit 0 (after `cargo fmt`; the fmt pass touched
  only `prose.rs` and `reflect_tests.rs` — mtime-verified).
- **File-size caps** — `view.rs` 992/1000 (untouched this round); reflect
  files mod.rs 409, prose.rs 518, paraphrase.rs 490, snapshot.rs 197,
  reflect_tests.rs 385; CLI files mod.rs 85, tests.rs 935 — all under.

## Executed vs read

**Executed:** the four mutations above (each reverted and md5-verified
identical), the four F1 legs and the two F2 pins and the F3 pin on the clean
tree and after each mutation, the full suite, clippy, fmt, and the real binary
end-to-end — `init`, seed, `reflect --dry-run` — on a fresh home; the
transient seed test was deleted and the temp home removed. The binary output
above is the record's evidence for the F2 gutter behavior on the real surface.

**Read (not re-executed):** lambo `4c6fc93` sources — `in_neighbors_typed`
returns id-ascending (why the F1 (b) set assert sorts both sides by `.0`),
`insert_concept` recreates the origin `Derives` edge on re-insert (why the
loser's incoming is exactly `[origin]` after `remove_node` + re-insert), and
`remove_node` clears incident edges (why the rerouted edge must be explicitly
re-upserted); `snapshot.rs`'s `derives` includes origin edges (why the returns
count for both concepts is origin + non-origin).

## Notes for the orchestrator

- The four `ingester/` working-tree changes present at session start
  (gemini-3.7-flash bump + `INGEST_LOCATION`) were **committed by the operator
  during this round** (`110781a` + `e63c23b`, see the intro) — not reverted,
  and not by me; I never touched that directory. HEAD is now `e63c23b`; the
  M12c implementation and this round's fixes remain uncommitted.
- F1's returns-tie is structural to the in-count-3 fixture (documented above):
  the (d) leg is defended at the comparator level, not the returns-key level.
- The round-2 record's lib count (565) measured 3 under the actual test fn
  count (570 in `src/`, of which 568 pass + 2 ignored after this round's +2).
