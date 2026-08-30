# M12a round-1 remediation

Fixes every finding in `m12a-round1.md` — both P1s, all four P2s, all eight P3s
— plus the out-of-scope `graph.db` mode noted at the end of that document, and
one defect the remediation run surfaced on its own. Base: `b1b6605`, branch
`main`.

Two findings were answered differently from the review's sketch, both because
the sketch's mechanism was measured and found not to work: R1-3's watchdog test
and R1-6's key-only collapse. Each is argued where it is fixed.

## Per-finding fixes

### P1-1 — the panels drew Lambo's provenance, absolute paths included

`threads` and `trickle` now pass every concept through one seam, `bookkeeping`,
and it is the only place in the module that filters by what a node *is*:

* content starting `document:` — the bootstrap ingester's own anchor, which
  gains a `Derives` edge from every turn that touched the document and so wins
  the one axis this panel ranks by;
* a concept at the source of a `Causal` or `Dependency` edge — `record_action`
  is the only writer of either in Lambo (`graph/action.rs`), so that is its
  action node, "Ingested file document:git:/Users/…".

`ConceptType` cannot separate either from a thought — an anchor is `Entity`, an
action node is `Resource`, and derived concepts are both — which is what the
review said and what the fixture now demonstrates. The action nodes are
collected in one pass over the edges rather than queried per concept, so the
filter costs one `HashSet` lookup a node.

Stated limit, written where the code is: an action recorded with **no**
produces/modifies/depends_on has no such edge and reads as an ordinary thought.
Nothing this product writes is shaped that way; the ingester always names what
it produced.

Pin: `provenance_is_not_a_thought_and_reaches_neither_panel` builds the review's
own corpus — one document read on three days, one fact reached twice, one
recorded ingest — and asserts both panels, then asserts that nothing on the
whole workspace contains `/Users/`.

*Mutation:* filter removed → the anchor takes rank 0 of "what keeps coming
back", exactly as the review measured.

### P1-2 — a day's log was `derive`'s joined concept list

The milestone picks the review's second option, out loud. `said` now drops a
turn whose words are the concepts it wrote: the whole prompt first, then every
piece of it split on `"; "`, tested against the set of concept contents in the
graph. One rule catches both writers — `derive` joins its concepts with that
separator, and `record_action` writes the action string as a concept — and the
single-concept case that contains a semicolon is caught by testing the whole
prompt before splitting it.

The test is over the graph's contents rather than the turn's own, because a turn
that re-derives an existing thought creates no concept: the reinforced concept's
`origin_interaction` is the *first* turn that reached it. The cost is one
over-approximation, written down beside the code: a person whose sentence is
exactly a concept is read as an echo.

On today's corpus this empties the panel, which is the honest answer for a
product that records no turn of its own, and it fills the moment something does.
The module header now says so in the same paragraph that refuses to write prose.

Pins: `a_turn_that_restates_what_it_wrote_is_not_a_days_log` (a two-concept
derive, a recorded action, and a real turn — only the third survives);
`a_single_concept_containing_the_separator_is_still_an_echo`; and the live
sqlite test below asserts the log is empty after a real `derive` round-trip,
which is where the semicolon used to reach the screen.

*Mutation:* echo test removed → the day's log reads
`"The ring holds 512 in flight; Overflow writers block"` and
`"Ingested file document:file:/Users/…"`, which is the review's pty capture.

### P2-3 — the lock order was a comment and nothing else

**Structural, and pinned as source order.** `of_graph` now takes `stats` ahead
of the graph, so `of_memory` is one expression —
`of_graph(&memory.stats(), &memory.graph().read(), now)` — and Rust's
left-to-right argument evaluation makes the *simplified* form the safe one. That
was the trap: the natural one-line rewrite is what reversed the order.

**The review's watchdog test was written, measured, and dropped.** Against the
reversed order it does not fail — it wedges: the leaked reader keeps the guard,
the writer thread it was racing blocks behind it, and `join` never returns, so
the whole suite hangs instead of reporting. Making the writer give up on a
bounded `try_write_for` fixes the hang and costs the catch — with the collision
window that small, the reversed order then passed 200 draws in 0.03 s. Both
outcomes were executed. A net that hangs instead of failing catches nothing, and
one that reports green on code that deadlocks the pane is worse, so neither
shipped. What shipped is `the_figures_are_read_before_the_graph_guard`, which
reads `of_memory`'s body and asserts the two acquires in order — deterministic,
instant, and the only form that can catch a fault whose failure mode is "never
returns".

*Mutation:* `let graph = memory.graph().read();` hoisted above the call → the pin
fails in 0.00 s with the reason named. (Under that mutation the rest of the
suite hangs, which is the fault itself.)

### P2-4 — SIGTERM and SIGHUP left the terminal and the lease behind

`tui::run` now takes both signals for the length of the session. The handler
does one relaxed store to a static; the loop reads it beside `app.running` and
leaves by the same path `Esc` takes, so the terminal is put back and
`tui_cmd::live` closes the session. Nothing is closed from the handler itself —
that is an async call and belongs on the path that already runs it. SIGINT is
deliberately not taken: raw mode turns off `ISIG`, so Ctrl-C is already a key.

Executed on a real pty (120x40, `MOOSHIK_HOME` on sqlite, a live session
holding the lease), with the handler and then without it:

| | alt screen left | exit | `mooshik stats` after |
| --- | --- | --- | --- |
| SIGTERM, with the handler | yes | 0 | **0**, immediately |
| SIGHUP, with the handler | yes | 0 | **0**, immediately |
| SIGTERM, handler removed | no | killed by 15 | 2 — "held by another writer (…#63959)" |
| SIGHUP, handler removed | no | killed by 1 | 2 — same, for a dead pid |

Pin: `a_termination_signal_asks_the_session_to_leave` raises each signal in
process. The install is a precondition rather than a step: under the default
disposition either signal kills the test binary, so a broken install fails
loudly.

`tui_cmd`'s header is corrected in the same commit, including the case that is
still true: a **panic** does not close the session, because Lambo's `Drop`
deliberately keeps the lease on an unclean drop and the lease is what makes a
crash safe. Closing a window is not a crash; that distinction is now the
paragraph.

### P2-5 — the +05:45 fixture could not see a DST regression

`view_clock_tests.rs` is new and exists for one reason, stated in its header: a
`DateTime<FixedOffset>` has no transitions, so every test next door agrees with
a zone collapsed to its offset. It carries `Eastern`, a hand-rolled `TimeZone`
with one real transition — 2026-11-01 06:00 UTC, −04:00 to −05:00 — which is
forty lines and needs neither `chrono-tz` nor a `TZ` that survives the runner.

`a_zone_is_not_an_offset_across_a_daylight_saving_change` draws on Tuesday
3 November and places four turns across the 25-hour Sunday: 00:30 EDT, both
01:30s, and the late-Saturday control. All three November instants land on
Sunday 1 and both 01:30s draw `01:30`.

*Mutation, the review's own:*
`let now = now.with_timezone(&now.offset().fix());` → **caught**, and the
failure is the M9 fault: "Half past midnight, summer time" moves onto Saturday
31 October.

### P2-6 — five thread slots and an extractor's paraphrases in three of them

Threads are now folded before the list is cut: candidates in strength order,
each either joining a cluster it belongs to or opening one, supports **unioned**
(a turn that derived two paraphrases came back once, not twice), day marks
merged, and the order settled after the fold rather than before it. The pool is
four times the panel, because a cluster whose copies each rank below five slots
is exactly the case a pool the width of the panel would drop before it could
fold. The row a cluster keeps is its strongest copy's.

`one_thought` has two legs. Lambo's own judgement first: a shared canonical key.
Then the embedding distance, at the radius post-M10 measured on the clean graph
through pgvector — 0.02, against a median nearest-neighbour distance of 0.031
and a median distance-to-everything of 0.353.

**This is the review's primary sketch rather than the cheaper one it offered,
and the difference is not stylistic.** A key-only collapse cannot fire for the
case R1-6 measured: Lambo's `insert_concept` refuses two non-`Observation`
concepts that share a canonical key (schema §4's partial `UNIQUE`), so an exact
key match between two live thoughts is unreachable through the write path, and
an extractor's paraphrase carries its own key by construction. Shipping only
that leg would have been a fix that cannot fail *and* cannot fire. The
embedding leg is display-only and says so where it is written: nothing is
merged, nothing is promoted, and the next tick asks again. Consolidating the
nodes is a write, and M12c's.

Stated residual: a copy with no vector yet — writes acknowledge before the
embedder runs, which is J3's design — is only reachable by the key leg and still
takes its own row.

Pins: `a_thought_said_two_ways_takes_one_row_and_keeps_both_days` (two
`Observation` concepts under one key, folded to five supports, outranking a
four-support thought, with both copies' day marks);
`two_thoughts_inside_the_paraphrase_radius_are_one_thought` pins the radius from
both sides (0.01 folds, 0.05 does not), and that a missing vector folds nothing.

*Mutation:* fold removed → three rows for two thoughts, and the pair drops below
the four-support thread.

### P3-7 — the refusal pointed at a document this product does not ship

`memory::facts` keeps Lambo's first sentence — the session, the holder, the
lease age, all operator-safe — and drops what it appends, because both of those
are true in Lambo's tree and false here: this binary exposes no forced takeover,
and `docs/reference/cli.mdx` is a file in Lambo's repository (`mooshik/docs`
holds one document). The remediation is now Mooshik's own, in
`memory.session_conflict`, and names keys this product actually binds. A message
with nothing appended is passed through whole.

Pins: two in `memory/mod.rs` (holder kept, `.mdx` and "takeover" gone, our own
sentence present; and the pass-through case), plus the updated CLI classifier
test. Also observed live, in the pty run above: the refusal a wedged lease
produced carried the new sentence and no path.

### P3-8 — the live test never touched the product store

`a_live_session_survives_the_store_and_fills_the_workspace` runs on
`StoreKind::Sqlite` against a real file, and does more than the review asked:
it writes, **closes, reopens**, and draws from what came back off the disk. The
in-memory store serializes nothing, so it could not have seen an adapter that
drops `prompt_text` or `event_time` — post-M10's own one-sentence lesson.

### P3-9 — the pass under the read lock, and one `Entry` per turn

Half fixed here, half explicitly deferred.

**Fixed:** a day's log is capped at `LOG = 256` lines, taken from the early end
because that is the end the panels draw from and there is no scroll past it —
`aside::entries` stops at the last interior row and no key reaches further. A
4 000-turn day used to build 4 000 `Entry` values, two `String`s each, for a
pane that draws about twenty rows.

**Deferred to M12b, deliberately and on the record:** the whole pass still runs
under one read guard, which is what the review flagged at a 250 ms tick. The
fix it names — building the view from a cloned slice rather than under the guard
— is a change to how the tick acquires the graph, and it belongs with the tick
that makes it matter. The remediation makes the pass no *heavier*: the two new
collections are one walk over the concepts and one over the edges, both
allocated once per draw rather than per node.

Pin: `a_days_log_is_bounded_by_what_a_panel_can_draw` (276 turns → 256 entries,
earliest kept, the day's bar still counting all of them).

### P3-10 — `week_dates` could produce seven identical days

The anchor is lifted to the first date with six days behind it, so
`checked_sub_days` cannot underflow and the seven dates are distinct by
construction. The invariant is now stated where it is guaranteed, naming what
rests on it: `screen::today::today_index` finds today by matching
`day_of_month`.

Pin: `a_week_at_the_end_of_the_calendar_is_still_seven_distinct_days` draws at
`NaiveDate::MIN` and asserts seven distinct dates, and that today matches
exactly one column — the last.

*Mutation:* clamp removed → one distinct date instead of seven.

### P3-11 — a flat week draws seven full bars

**Behaviour kept, argument written, answer pinned.** The height is a share of
this week and carries no absolute meaning — that is the same sentence that keeps
a quiet week off the baseline, and it is what makes seven single-turn days each
the whole of their own week. Capping a flat week would put an absolute judgement
back into exactly one case and leave `[9; 7]` two rows below a week with one
quieter day. `bar_level`'s doc now makes the ceiling argument at the length the
floor's was already made, and
`a_flat_week_is_drawn_flat_at_the_top_of_its_own_scale` asserts `███████` at one
turn a day and at nine, so it is a decision rather than a side effect.

### P3-12 — dynamic calendar keys with no completeness pin

`every_day_of_a_year_has_a_name_in_every_table` walks a leap year and a week
past it through `long_date`, `short_date`, `day_head`, `day_month` and
`week_label`, and asserts twelve months and seven weekdays were reached with no
empty rendering and no unresolved placeholder.

*Mutation:* `tui.month.5` deleted → panics in the draw path with the key named,
which is the shipping failure the review described.

### P3-13 — `days()` re-derived `about_time` past its own memo

The placement travels with the turn: `logs` holds `(Placed, &Interaction)`, the
comparator reads `Placed::at`, and so does the clock string. `about_time()` is
now called in exactly one place in the module — `placements`, which exists to
call it once.

### P3-14 — `short_scope` was the long form, and `scope` said nothing about range

`health` takes the day the earliest turn is *about* (never the flush stamp) and
writes both forms from `en.toml`: `"214 things remembered, back to 15 June"` and
`"214 remembered"`. A graph with no far end keeps the short true sentence rather
than inventing a date. `date_day_month` is its own key because "back to Friday
21 August" names a weekday nobody asked about.

Pin: `the_scope_says_how_far_back_the_session_goes_and_the_short_form_does_not`,
including the empty-graph case.

*Mutation:* `short_scope: scope` → caught.

## Out of scope, in its own commit

### `mooshik init` created `graph.db` at 0644

sqlx opens the sqlite store with `create_if_missing`, so the file took the
process umask while the config, the vault and the marker beside it are all
`0600`. `claim_local_store` brings the database into existence first, through
`secure_path::ensure_private_file_at` — the same primitive the config and the
vault use — so it never exists at a wider mode, and one already there is
repaired on the next run exactly as `HomeLayout::init` repairs the others.
SQLite gives `-wal` and `-shm` the database's own mode, so both follow. Stores
that name no local file (Postgres, and sqlite's in-memory spellings, whose
grammar is mirrored from `SqliteStore::is_in_memory_uri`) have nothing to claim.

Pins: `the_local_database_is_created_and_repaired_private` (created 0600,
widened to 0644 and repaired, still 0600 after a real session writes to it) and
`a_store_with_no_local_file_is_provisioned_without_one`. Also observed through
the real `mooshik init` in the pty harness: `graph.db mode: 600`.

*Mutation:* `claim_local_store` call removed → "a fresh database is
world-readable".

### A sandbox name that could collide, found by this run

`tools::scratch::tests::output_is_capped` failed once during a full parallel
run: `Sandbox::create` named the directory from the pid and `subsec_nanos`, and
macOS's realtime clock advances in *microseconds*, so two sandboxes opened in
the same microsecond drew the same name and the loser got `File exists`. That is
the same fault the vault fixtures hit on Darwin, in product code rather than a
fixture — reachable from two concurrent scratch tool calls in one process. A
process-wide counter joins the pid and the clock: the pid separates processes,
the counter separates two calls inside one, and the clock still separates this
run from a directory a killed process left behind.

Pin: `two_sandboxes_opened_in_the_same_instant_are_two_directories`.

## Mutation summary

| Mutation | Result |
| --- | --- |
| `of_graph` collapses the reader's zone to its offset (the review's own) | **caught** — the DST turn moves to 31 October |
| `of_memory` takes the graph guard before the figures (the review's own) | **caught** — source-order pin fails immediately |
| `bookkeeping` filter dropped from both panels | **caught** — the `document:` anchor takes rank 0 |
| the derive-echo test dropped from `said` | **caught** — the joined prompt and the ingest line reach the log |
| the thread fold dropped | **caught** — three rows for two thoughts |
| `week_dates` clamp dropped | **caught** — one distinct date instead of seven |
| `log.truncate(LOG)` dropped | **caught** — 276 entries |
| `short_scope` set to the long form | **caught** |
| `tui.month.5` deleted from `en.toml` | **caught** — panics with the key named |
| `claim_local_store` call dropped | **caught** — 0644 |

Every mutation transient; `git status --porcelain` verified empty after each.

## Gates

* `cargo test` → **536 lib + 1 integration**, 0 failed, 2 ignored (518 before,
  +18 pins).
* `cargo clippy --all-targets --all-features` → clean.
* `cargo fmt --check` → clean.
* File-size cap → clean. `view_tests.rs` crossed 1000 lines during this work and
  was split by subject, not by size: `view_clock_tests.rs` holds the zone and
  calendar tests (it needs a `TimeZone`, and the file it left is written against
  a fixed offset), `view_session_tests.rs` the tests that hold a `Memory`.
