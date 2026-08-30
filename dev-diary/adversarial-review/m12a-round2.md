# M12a round 2 — adversarial re-verification of the round-1 remediation

Reviewed at `39e4c9f`, branch `main`, tree clean before and after. Scope: every
one of the fourteen findings in `m12a-round1.md`, the three places
`m12a-remediation-round1.md` answered differently from the review's sketch, the
two out-of-scope fixes the remediation added (`graph.db` at 0600, the scratch
sandbox name), and the new code the remediation introduced — the `bookkeeping`
seam, `said`'s echo test, the thread fold, the signal handler, the `LOG` cap and
`claim_local_store`. Read in full, then attacked against the pinned lambo crate
at `4c6fc93` and by mutation. All transient edits reverted; `git status
--porcelain` verified empty after every one.

## Verdict

**REMEDIATE** — 1 × P1, 1 × P2, 4 × P3.

The fourteen findings are genuinely fixed. Every mutation the remediation
claims reproduces, and I found no fix that fails to hold. All three
disagreements are **upheld on the merits**, and the R1-3 one is upheld by
measurement: the watchdog the review asked for does not fail against the
reversed order, it wedges — I ran it, and the test binary was still holding the
guard three minutes later.

What is wrong is the **edge of two of the fixes, and the pins that were built to
not see it**. The 0600 fix repairs `graph.db` and leaves `graph.db-wal` beside it
at 0644, on exactly the installs the fix exists for; the WAL survives a clean
close, keeps growing, and carries concept content in cleartext. And the scratch
sandbox pin passes with the fix reverted — three runs out of three. Both are the
post-M10 hazard in its purest form: a fixture that agrees with the gap.

## What held up under attack

* **Both P1s are closed, and their pins bite.** Dropping the `bookkeeping`
  filter puts `document:file:/Users/neom/notes/windpipe-design.md` at rank 0 of
  "what keeps coming back"; dropping `said`'s echo test puts
  `"The ring holds 512 in flight; Overflow writers block"` and
  `"Ingested file document:file:/Users/…"` back into the day's log, and takes
  four tests down with it including the live sqlite one. Both executed.
* **The `bookkeeping` premise is true of lambo at the pin.** I enumerated every
  `EdgeType::Causal` / `EdgeType::Dependency` construction in the pinned tree:
  the only one outside a `#[cfg(test)]` module is
  `graph::action::plan`. Every edge it plans runs `action_node -> target`
  (`action.rs:230-262`), so `edge.source` is the action node and nothing else —
  the filter cannot catch a *produced* or *depended-on* concept by mistake.
* **`Memory::stats` really does take the graph lock** (`lambo memory.rs:2144`,
  `let g = self.graph.read();`), so the ordering `of_memory` protects is
  load-bearing, and the structural fix protects it.
* **The zone pin is a zone.** `Eastern` is a real `TimeZone` with one real
  transition, and the review's own mutation now fails there and only there:
  "Half past midnight, summer time" moves onto Saturday 31 October.
* **The fold's second leg is measured, not asserted.** Executed a radius chain:
  A–B 0.0098, B–C 0.0091, A–C 0.0377 against `PARAPHRASE = 0.02`. Unembedded
  concepts fold nothing, a mixed embedded/unembedded pair folds nothing, and the
  clustering is representative-anchored — every copy a row absorbs is inside the
  radius of the row that is drawn, which is what makes the row a true statement.
* **The signal handler is async-signal-safe, and the claim above it is right.**
  `note_signal` does one `Relaxed` store to a `static AtomicBool`, which is
  lock-free on every supported target. `sa_flags` is zero, so no `SA_RESTART` —
  and crossterm 0.28.1's `UnixInternalEventSource::try_read` retries an EINTR
  poll with `timeout.leftover()` (`event/source/unix/mio.rs:78-84`), exactly as
  `event_loop`'s doc claims, so the flag is seen within one `TICK` and no
  partial escape sequence is left on the terminal (`write_all` and
  `BufWriter::flush_buf` both retry `Interrupted`). The store/check race costs
  at most one extra draw.
* **`claim_local_store` does not truncate an existing database.**
  `secure_path::ensure_private_file_at` takes the `Opened::Existing` arm and
  chmods only; the bytes are written on the `Created` arm alone. The live sqlite
  test writes, closes, reopens through `resolve_product` — which now claims —
  and reads its own concepts back, which is the pin that would have caught a
  truncating repair.
* **The `LOG` cap keeps the end that is drawn.** `days()` filters through `said`
  *before* pushing, sorts ascending, then truncates — so the cap never spends a
  slot on an unquotable turn, and `aside::entries` (`aside.rs:95-122`) iterates
  from `log[0]` and stops at the panel's last interior row. The bar still counts
  the whole day.
* **The remediation's "no heavier" claim is true.** Measured at round 1's own
  shapes, debug build: 1 000 turns / 400 concepts / 1 999 edges → **6.5 ms**
  whole pass, of which the two new collections are **0.28 ms**; 4 000 / 1 500 /
  7 999 → **18.2 ms**, of which **0.95 ms**. Round 1 measured 4.6 ms and 19.3 ms
  for the same shapes, so the added walk over the edges is ~5% and inside the
  noise.
* **The scratch counter itself is correct.** The pid separates processes, the
  counter separates two calls inside one, and `AtomicU64` will not wrap in this
  universe. It is only the *pin* that is wrong (R2-2).
* **The M12b deferral is on the record.** `m12a-remediation-round1.md` §P3-9
  states it in as many words — the whole pass still runs under one read guard,
  and the cloned-slice fix belongs with the tick that makes it matter. That
  satisfies the criterion. Noted for the record: `PLAN.md`'s M12b bullet says
  only "the cost of a rebuild … is the thing to measure first" and does not carry
  the guard-duration item, and no `M12b` marker exists anywhere in `src/`.
* **Gates.** `cargo test --locked` → **536 lib + 1 integration**, 0 failed, 2
  ignored (both pre-existing and legitimately marked; no new `#[ignore]`).
  `cargo clippy --all-targets --all-features` clean. `cargo fmt --check` clean.
  Every touched file under the 1000-line cap (largest: `view_tests.rs` 871,
  `view.rs` 868). CI green at `39e4c9f` (run 33323872980, success).

## Findings

### P1

**M12a-R2-1 — The 0600 fix repairs the database and leaves the write-ahead log
world-readable, on exactly the installs it exists for.**

`claim_local_store` claims `graph.db` and nothing else. The remediation's stated
reason is that "SQLite gives `-wal` and `-shm` the database's own mode, so both
follow" — which is true of files SQLite **creates**, and false of files that are
already there. On the upgrade path — a home written by any build before
`2946c40`, where the database *and* the side files SQLite gave the database's own
mode are all 0644 — the repair reaches one of the three.

Executed, through `provision` and a real session:

```
[after provision]      graph.db 600  graph.db-shm 600  graph.db-wal 600
[after close]          graph.db 600  graph.db-shm 600  graph.db-wal 600   90672B
[pre-fix state]        graph.db 644  graph.db-shm 644  graph.db-wal 644   90672B
[after repair run]     graph.db 600  graph.db-shm 644  graph.db-wal 644   90672B
[after a real session] graph.db 600  graph.db-shm 644  graph.db-wal 644  173072B
readable: 'a second thought' present in the 0644 wal = true
```

Three things make this a P1 rather than a note.

1. **The exposed file is the same content.** WAL frames are raw page images; the
   probe found a concept's own words in the 0644 file by a plain substring
   search. "Everything the user has ever remembered is world-readable on a shared
   machine" — round 1's sentence — is still true of a pre-fix home, in a file
   that is *growing*, not a stale remnant.
2. **It does not heal.** Measured above: the WAL survives a clean `close()` and
   is still there, larger, after the next session. It is not deleted and
   recreated, so it never inherits the repaired mode. This repository's own
   working directory has a `mooshik.db-wal` at 0644 holding 111 272 bytes with no
   `mooshik.db` beside it — the steady state, not an accident.
3. **The pin was built so it cannot see it.** `the_local_database_is_created_and_repaired_private`
   creates a fresh home, where the side files are born from an already-0600
   database, and then widens and re-checks `graph.db` alone. It exercises the
   only case that was never at risk and asserts nothing about the two files that
   are.

*Remediation.* Claim the side files the same way the database is claimed —
`graph.db-wal` and `graph.db-shm` beside it, repaired when present and skipped
when absent (they are SQLite's own names, derived from the database path, and
`HomeLayout::init` already repairs a set of files rather than one). Then widen
the pin: set all three to 0644, run `provision`, and assert all three come back
0600. If claiming a file SQLite owns is judged unsafe, the alternative is to
narrow the *directory* — but the file modes are what the rest of the home is
held to, and the doc's sentence about `-wal` and `-shm` following has to become
true or go.

### P2

**M12a-R2-2 — The scratch-collision pin passes with the fix reverted, and its
comment describes a mechanism nothing implements.**

`two_sandboxes_opened_in_the_same_instant_are_two_directories` calls
`Sandbox::create()` twice in a row and asserts the two paths differ. Each call
samples `SystemTime::now()` *itself*, and a `create_dir` syscall sits between the
two — tens of microseconds on Darwin, against a realtime clock that advances in
one. The two clock readings therefore differ on their own, with or without the
counter.

Mutation applied: the name reverted to the pre-fix `pid` + `subsec_nanos`, with
the counter still incremented and discarded so nothing else moved.

**Three runs, three passes.** The pin is green on the code it exists to guard
against.

Its doc comment says why it should not be: "The clock is sampled once here so the
test does not have to win a race to observe the fault it is guarding." Nothing in
the test samples the clock, once or at all. That sentence describes a test that
was not written.

The fault itself is real and the fix is correct — a full parallel run surfaced it
and the counter closes it — which is exactly what makes an inert pin worth a P2:
the next person to simplify `Sandbox::create` back to two parts will be told the
suite agrees with them.

*Remediation.* Make the test observe the fault it names: sample the clock once
and build both names from that one instant (extract the naming into a function
that takes the instant, and let `create` call it with `now()`), then assert the
two names differ. That is the shape the comment already claims. A weaker but
honest alternative is to assert the *name* contains three parts and delete the
sentence about sampling.

### P3

**M12a-R2-3 — `tui_cmd`'s header still advertises the override the remediation
deleted.** R1-7 quoted this sentence by name: the header says the refusal
carries "Lambo's own conflict sentence, which names the holder and the
override". The remediation fixed the message and left the sentence
(`tui_cmd.rs:21-22`). It is now false twice over: the rendered refusal is
Mooshik's own template with Lambo's facts interpolated, not Lambo's sentence,
and `memory::facts` cuts the override out on purpose — `a_session_conflict_names_the_holder_and_no_page_this_product_does_not_ship`
asserts `!rendered.contains("takeover")`. `39e4c9f` went back for two stale
comments in `view.rs`; this is the third, and it is the one the round-1 finding
pointed at.

**M12a-R2-4 — The signal handler outlives the session it was installed for,
including the close it exists to reach.** `leave_on_signals` is never
uninstalled and `run()` restores no disposition, so from the first draw until the
process ends, SIGTERM and SIGHUP do nothing but set a flag nobody reads any more.
The window that matters is `tui_cmd::live` lines 71-76: `run()` returns, then
`runtime.block_on(memory.close())` — the call the whole fix exists to reach — and
a `kill` on a session whose close is stuck on a wedged store is now a no-op where
it used to end the process. The doc's own justification is wrong about the
ordering: "there is nowhere to put it back on: the loop leaves and the command
returns" — the command has not returned, it is closing. The same holds in the
test binary: once `a_termination_signal_asks_the_session_to_leave` has run, a
plain `kill` on the suite is ignored for the rest of the run, which is worth
knowing in a repo that has just measured a fault whose failure mode is "never
returns". Restore the previous disposition around the loop, or narrow the
sentence to what the code does.

**M12a-R2-5 — `said`'s written limit is narrower than `said`'s rule.** The doc
admits one over-approximation: "a person whose sentence is *exactly* a concept in
the graph is read as an echo". The second leg is wider than that — a prompt is
dropped when **every piece of it, split on `"; "`, is a concept** — so a person's
multi-clause sentence disappears without ever being a concept itself. Executed
against a hand-built corpus: with `"Ship it"` and `"it rained"` both in the
graph as separate facts, a real turn saying `"Ship it; it rained"` leaves the
day's log **empty**, alongside the documented case. The rule is defensible; the
sentence describing it should cover the case it actually catches.

**M12a-R2-6 — `bookkeeping`'s stated limit is one-sided.** The doc names the
false negative ("an action recorded with no targets has no such edge and reads
as an ordinary thought") and not the false positive. `record_action` resolves its
action string through `canonicalize` and **reuses an existing node on a key
match** (`lambo graph/action.rs:308-321`, `CanonicalizeResult::Matched { node }`),
so an action whose text canonicalizes onto a thought the user already had turns
that thought into the action node — and `bookkeeping` then hides a real thought
from both panels, permanently, because the `Causal` edge never goes away.
`lambo_record_action` is on this product's own MCP surface, so the action string
is not always the ingester's. One sentence beside the other limit.

## The three documented disagreements

**R1-3 — structural fix instead of the watchdog. UPHELD, by measurement.** The
remediation's claim is that the watchdog wedges rather than fails. I built it:
a real sqlite session, a thread hammering `graph().write()` in a loop, and 200
`of_memory` draws. At the shipped order it finishes in **17 ms** with the writer
taking the lock 1 546 times. At the reversed order it printed `PROBE draw 0` and
then **never returned** — cargo's own "has been running for over 60 seconds",
still wedged when I killed it minutes later. Round 1's literal sketch ("call
`stats()` on the same thread, and fail if it has not returned inside a second")
is unimplementable: the thread that must report is the thread that is blocked.
The one shape the remediation did not try — a leaked victim thread plus
`recv_timeout` on the main thread — *would* catch it deterministically, at the
cost of a permanently wedged thread and an unclosable `Memory` holding a lease
for the rest of the run; and it would catch nothing the source-order pin does not
already catch instantly. The structural half is real: `of_graph(&stats, &graph,
now)` plus left-to-right argument evaluation makes the one-expression form safe,
and the mutation that hoists the guard fails the pin in 0.00 s with the reason
named. *Recorded for M12b:* `of_graph` is `pub` and the pin reads only
`of_memory`'s body, so a future caller that takes the guard and then asks for the
figures is caught by neither — worth a line in the tick's own review.

**R1-6 — display dedup instead of key-only collapse. UPHELD.** Verified against
lambo at the pin: `Graph::insert_concept` (`graph/graph.rs:461-479`) rejects two
non-`Observation` concepts sharing a canonical key, with the schema §4 partial
`UNIQUE` written out beside it and its own test. An extractor's paraphrases are
`Entity`, so a key-only collapse could not have fired for the case R1-6 measured
— the remediation is right that it would have been a fix that cannot fail and
cannot fire. The key leg is *not* dead, though: Observations are exempt and may
shadow an Entity key, which is the demoted-chunk case the shipped test uses.
Probed the shipped mechanism at the 0.02 radius:

* **Unembedded concepts fold nothing.** `one_thought` returns on
  `(Some, Some)` or not at all. Pinned from both sides in
  `two_thoughts_inside_the_paraphrase_radius_are_one_thought`.
* **A mixed embedded/unembedded pair folds nothing**, by the same branch — the
  residual the remediation states.
* **A radius chain is not transitive, and that is the defensible answer.**
  Executed with A–B 0.0098, B–C 0.0091, A–C 0.0377: with A strongest the panel
  draws two rows (`A`, `C`); with B strongest it draws one (`B`). The fold is
  anchored on the strongest copy and compares only against it, so every copy a
  row absorbs is inside the radius of *the row that is drawn* — which is what
  makes the row true either way. It is order-dependent but deterministic:
  `strongest_first` is a total order, so the same graph draws the same thing on
  every tick. The module doc already says the strongest candidate keeps the row
  and the rest fold into it, which is the property this rests on. No finding.
* One limit worth recording: the embedding leg is exercised only through
  `one_thought` directly, never through `threads()` — a hand-built `Graph` has no
  embedding contract, so the end-to-end fold test uses the key leg. Both legs are
  pinned; only one is pinned through the panel.

**R1-11 — flat week stays seven full bars. UPHELD.** The argument is sound
against `model.rs`, which promises nothing the flat week breaks — `Load`'s own
documentation settles it: `level` "is a drawing instruction rather than a
measurement", and the ribbon "is read as a shape" with `1i` ruling the number off
the screen. Nothing in `model.rs` or the artboards says a bar's height is
comparable across weeks, and `bar_level`'s own first paragraph — relative, not
absolute — is the premise a ceiling would contradict. A quiet flat week and a loud flat week
drawing identically is the same statement as a quiet week not flattening onto the
baseline, which is the half that was already defended. And the alternative is
worse in a way that is checkable: a cap would put two rows between `[9,9,9,9,9,9,9]`
and `[9,9,9,9,9,9,8]`, which is a discontinuity nothing in the design asks for.
`a_flat_week_is_drawn_flat_at_the_top_of_its_own_scale` asserts `███████` at one
turn a day and at nine, so it is a decision on the record. The one thing the
ribbon cannot tell you is how loud the week was — but it never could, and the
status bar's `scope` is where a count belongs.

## Mutation-tested pins

Every mutation transient; `git status --porcelain` verified empty after each.

| Mutation | Pin | Result |
| --- | --- | --- |
| `bookkeeping` filter dropped from both panels | `provenance_is_not_a_thought_and_reaches_neither_panel` | **caught** — the `document:` anchor takes rank 0 |
| the derive-echo test dropped from `said` | 4 tests incl. the live sqlite one | **caught** — the joined prompt and the ingest line reach the log |
| `of_memory` takes the graph guard before the figures | `the_figures_are_read_before_the_graph_guard` | **caught** — fails in 0.00 s with the reason named |
| the same, run under a hammering writer (round 2's own) | *probe, not committed* | **wedged** — `PROBE draw 0`, then never returned; killed after minutes |
| `of_graph` collapses the reader's zone to its offset | `a_zone_is_not_an_offset_across_a_daylight_saving_change` | **caught** — the DST turn moves to 31 October |
| the thread fold dropped | `a_thought_said_two_ways_takes_one_row_and_keeps_both_days` | **caught** — three rows for two thoughts, pair below the four-support thread |
| `log.truncate(LOG)` dropped | `a_days_log_is_bounded_by_what_a_panel_can_draw` | **caught** — 276 entries |
| `week_dates` clamp dropped | `a_week_at_the_end_of_the_calendar_is_still_seven_distinct_days` | **caught** — 1 distinct date instead of 7 |
| `tui.month.5` deleted from `en.toml` | `every_day_of_a_year_has_a_name_in_every_table` | **caught** — panics in the draw path (`text/mod.rs:36`) |
| `short_scope` set to the long form | `the_scope_says_how_far_back_the_session_goes_and_the_short_form_does_not` | **caught** |
| `facts()` passes the whole detail through | `a_session_conflict_names_the_holder_and_no_page_this_product_does_not_ship` | **caught** |
| `claim_local_store` call dropped | `the_local_database_is_created_and_repaired_private` | **caught** — 0644 |
| the in-memory guard dropped from `claim_local_store` | `a_store_with_no_local_file_is_provisioned_without_one` | **caught** |
| `leave_on_signals` made a no-op | `a_termination_signal_asks_the_session_to_leave` | **caught** — the test binary is killed by signal 15 and cargo reports the failure |
| the scratch name reverted to `pid` + `subsec_nanos` | `two_sandboxes_opened_in_the_same_instant_are_two_directories` | **SURVIVES** — 3 runs, 3 passes (P2, R2-2) |

The remediation added a net eighteen pins, and every one of them was
bite-tested. Sixteen were bitten by the mutations above; three more are
bite-proof by construction and needed none —
`two_thoughts_inside_the_paraphrase_radius_are_one_thought` asserts both sides of
the radius *and* the no-vector case, so it cannot survive a changed constant;
`a_conflict_with_nothing_appended_keeps_all_of_it` is a direct assert on
`facts`; and `a_flat_week_is_drawn_flat_at_the_top_of_its_own_scale` asserts the
decision at one turn a day and at nine, so any ceiling fails it. One survives its
mutation, and that is R2-2.

## What was executed vs. only read

**Executed.** `cargo test --locked` (536 lib + 1 integration, 0 failed, 2
ignored); `cargo clippy --all-targets --all-features`; `cargo fmt --check`; the
1000-line cap by hand. Fourteen distinct mutations, listed above (the reversed
lock order was run twice, against the source pin and against the race), each
reverted and the tree verified clean. Four transient probes, written, run and
deleted: the
reversed-order contention race against a real sqlite session (control and fault);
the `said` false-positive corpus; the paraphrase radius chain at three tilts with
a stamped embedding contract; and the WAL mode probe through `provision`, a live
`derive`, a close, a simulated pre-fix home and a repair run, ending with a
substring search of the 0644 file for a concept's own words. A cost measurement
of `of_graph` at round 1's two shapes, in debug and release, with the two new
collections timed separately. The lambo tree at `4c6fc93`, read by `git show`
rather than from the working copy: every `Causal`/`Dependency` construction
enumerated across all of `src/`, `insert_concept`'s canonical-key rejection,
`record_action`'s edge planning and its `resolve`, `Memory::stats`'s graph
acquire, and the lease-held message the `facts` cut is aimed at. crossterm
0.28.1's EINTR retry, read from the vendored source. CI status at HEAD via `gh`.

**Read, not executed.** The pty behaviour of SIGTERM/SIGHUP against a live lease
(the remediation's own table; `a_termination_signal_asks_the_session_to_leave`
and mutation M8 establish the handler half by a route that could be executed).
The Postgres path of `claim_local_store` (it returns early on a non-sqlite kind;
sqlite was executed). `write_all`/`BufWriter::flush_buf` retrying `Interrupted`
— std's documented behaviour, traced rather than instrumented.

## Outside this round, noted in passing

`Sandbox::create` makes its directory with `fs::create_dir` and writes the
script with `fs::File::create`, so both take the process umask — 0755 and 0644 on
an ordinary account — in the world-readable `/tmp`. Model-authored code and
whatever it leaves behind are therefore readable by every account on the machine.
Pre-existing, unrelated to M12a, and not one of the fourteen; recorded here
because the round that made the graph 0600 is the round that establishes the
standard.
