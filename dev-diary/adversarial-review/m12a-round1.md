# M12a round 1 — adversarial review of the workspace view

Reviewed at `cf3dcbb` (implementation) and `1c75c87` (PLAN.md), branch `main`,
tree clean before and after. Scope: `src/memory/view.rs`, `view_tests.rs`,
`src/cli/tui_cmd.rs`, the `runtime()` extraction in `src/cli/mod.rs`,
`Week::day_heads` and `screen/week.rs`'s header, the `en.toml` additions, and
the `tui/mod.rs` deletion of `live`/`from_stats` — read in full, then attacked
against the pinned lambo crate at `4c6fc93` (what the product actually writes)
and against a live sqlite session seeded through `mooshik serve`'s own MCP
surface. Every claim below is executed or traced from source. All transient
edits reverted; the scratch home was deleted.

## Verdict

**REMEDIATE** — 2 × P1, 4 × P2, 8 × P3.

The arithmetic is right. `about_time` discipline is right, DST is right, the
lease refuses correctly, the lock order is genuinely load-bearing and the
sqlite round-trip works. What is wrong is **what the panels are pointed at**:
on the only corpus this product can produce, "What keeps coming back" lists
absolute paths from the reader's home directory, "Just remembered" spends two
of five slots on `Ingested file …` bookkeeping, and a day's log is
`Memory::derive`'s `concepts.join("; ")`. The module header spends four
paragraphs refusing to dress a mechanical string as prose, and then fills the
one prose field it did fill with a mechanical string.

Both P1s are one-corpus-deep. Neither is visible from `view_tests.rs`, because
that suite hand-writes English sentences into `prompt_text` and the one live
test asserts the log is *non-empty* without ever reading a line of it. That is
the post-M10 hazard exactly: the fixture agrees with the bug.

## What held up under attack

* **DST is correct, and it was the first thing I tried to break.** Executed at
  `TZ=America/New_York` over the 2026-11-01 fall-back (a 25-hour day, 01:30
  local twice): both 01:30s land on Sun 1, `31 Oct 23:30 EDT` on Sat 31,
  `1 Nov 23:30 EST` on Sun 1, `2 Nov 00:30 EST` on Mon 2. Executed again at
  `TZ=Asia/Kathmandu` (+05:45). `day_index` converts to a local *date* and
  `week_dates` does calendar arithmetic on `NaiveDate`, so nothing rounds.
* **The UTC/local seam.** One turn stamped `2026-08-26 23:59 UTC`, drawn at
  five offsets: `+05:45 → Thu 27 05:44`, `+13:00 → Thu 27 12:59`,
  `00:00 → Wed 26 23:59`, `−05:00 → Wed 26 18:59`, `−11:00 → Wed 26 12:59`.
  Correct at every one.
* **`about_time` is the single home of the rule.** `Concept::created_at` is
  never read anywhere in `view.rs` (`rg` over the file: the only hits are the
  doc comments explaining why not). `about()` is the only reader of
  `origin_interaction`, and `threads` deliberately does not call it — a
  thread's marks come from its *supports'* about-times, which is a different
  and correct question. Executed: a 2015-stamped turn flushed just now draws on
  its own historical day when the clock is set to 2015, and nowhere when it is
  set to 2026.
* **The recurrence count really is distinct interactions.** `in_neighbors_typed`
  collects from `HashMap<NodeId, HashMap<EdgeType, HashSet<NodeId>>>` — a set,
  so an interaction that re-derives the same concept reinforces one edge and
  counts once. The `RETURNS = 2` docstring's claim is true, not hopeful.
* **The lock order is load-bearing, not folklore.** Executed: a thread that
  takes `memory.graph().read()` first and calls `memory.stats()` second, with a
  writer queued between the two acquires, **had not completed after 3 s**.
  `Memory::stats` does take `self.graph.read()` (lambo `memory.rs:2142`), and
  `parking_lot`'s read lock is not recursion-safe against a queued writer. The
  ordering in `of_memory` is correct. (It is also unpinned — P2, R1-3.)
* **The lease refusal is exactly what the module doc claims, minus one word.**
  Executed against a live `mooshik serve` holder: `mooshik tui` exits **2** and
  prints one sentence naming session, agent, host, pid, lease age and a
  remediation. No DSN, no store path, no credential, no error chain —
  `Failure::rendered` prints the top-level `Display` and `MemoryError::Backend`
  never renders its source. Verified on a pty and off one.
* **The store round-trip works.** `prompt_text` and `event_time` are persisted
  and reloaded by both the sqlite and Postgres adapters, and a full
  `load_session` is unwindowed — so `placements` really does see every
  interaction and `returns` cannot be inflated by an absent support. Confirmed
  end to end: a graph written through MCP, closed, reopened by `memory::open`,
  and rendered with all four turns per day on the right days.
* **A draw that *fails* does release the lease.** `let drawn = draw(workspace);`
  is bound, not `?`-ed; `memory.close()` runs before `drawn?`. Executed on the
  no-tty path: `mooshik tui | cat` reaches `start()`, fails, and the next
  command opens the session immediately. (A draw that *panics* does not — see
  R1-4.)
* **`runtime()` changed no command's behaviour.** `runtime()?.block_on(fut)`
  keeps the `Runtime` alive as a temporary for the whole statement, which is
  what the inlined builder chain did; `Runtime::drop` still blocks on shutdown
  at the same point. In `live`, `memory` is declared after `runtime` and so
  drops first — the comment is right and the order matters.
* **`en.toml` is clean.** All twelve new keys and both new tables are
  referenced; `week_day_header` survives as the demo fallback and is still
  read. No orphans. Nothing new reaches a user-facing `Display`.
* **The week's calendar labels.** Executed across a month boundary
  (`27 August - 2 September`), a year boundary (`27 December - 2 January`) and
  a leap-year February (`24 February - 1 March`), with `day_heads` following
  the week in every case.
* **The any-activity floor works at the extreme.** `[1000, 0, 0, 0, 0, 0, 1]`
  draws `█▁▁▁▁▁▂` — one turn against a thousand still clears the empty day's
  glyph.
* **Gates.** `cargo test --locked` → 518 lib + 1 integration, 0 failed, 2
  ignored. `cargo clippy --all-targets --all-features` clean. `cargo fmt
  --check` clean. Every touched file under the 1000-line cap (largest:
  `week_tests.rs` 883, `view_tests.rs` 646).

## Findings

### P1

**M12a-R1-1 — "What keeps coming back" and "Just remembered" draw Lambo's
provenance bookkeeping, absolute home paths included.**

`threads` and `trickle` iterate `graph.concepts()` with no filter on
`concept_type` and no filter on content. On the only corpus this product can
produce, that is not a list of thoughts.

Executed. A sqlite session seeded through `mooshik serve`'s MCP surface with
the M8 ingester's own shape (two documents × two facts × three days, via
`lambo_derive` with `parent_of` anchors and `lambo_record_action`), then read
back through `memory::open` + `of_memory`:

```
=== what keeps coming back ===
  0. "Overflow writers block instead of dropping messages"
  ...
  4. "document:file:/Users/narayan/Documents/work/notes/windpipe-design.md"
=== just remembered ===
  . "Ingested file document:file:/Users/narayan/Documents/work/notes/windpipe-design.md"
  . "Ingested file document:git:/Users/narayan/work/lambo#4c6fc93"
  . "Overflow writers block instead of dropping messages"
```

Two of five "just remembered" slots, and one of five thread slots, are
provenance. On a wider corpus it is worse, not better: `Memory::derive` gives
every `ParentOf` parent a `Derives` edge from the interaction that read it, so
a document anchor accumulates **one support per turn that touched it** — which
is precisely why post-M10's clean run measured that the *only* nodes reaching
Venerable were `document:file:…` resources ("the only part of the corpus with
stable identity is the only part that climbed"). The panel ranks by exactly
that count. Reproduced in-memory with a second corpus where the anchors took
**ranks 0 and 1**, above every fact.

Three separate things are wrong with this:

1. `model.rs`'s own rule — "Nothing from the engine underneath reaches this
   type either — no sessions, no nodes, no relevance" — is broken by a string
   whose entire content is engine provenance.
2. It is a path disclosure on a pane whose whole design premise is that it is
   left open beside your work. `document:file:<absolute path>` and
   `document:git:<repo>#<sha>` name the reader's home directory on screen.
   M9-round-1's P3-7 already flagged this ref shape as a hazard when it was
   only reaching a grading file.
3. The flagship panel is crowded out by rows that are not memory. Post-M10
   measured 53 documents × 2 provenance nodes on the real graph; a five-slot
   panel does not survive that.

*Remediation.* Filter provenance out of both lists in `view.rs`, at one seam
and with the reason written down: skip concepts whose content starts with the
`document:` provenance prefix, and skip `record_action`'s action concepts.
`ConceptType` alone is not sufficient — an anchor is `Entity` (derive's
`PARENT_OF_CONCEPT_TYPE`) while an action node is `Resource`, and legitimate
concepts are both — so content prefix is the honest discriminator, and it
belongs beside the `RETURNS` constant with the same kind of comment. If the
prefix is judged too fragile a contract to depend on, the alternative is to ask
Lambo for a provenance flag; either way an absolute path must not reach a
`Thread::summary` or a `Trickle`.

---

**M12a-R1-2 — A day's log is `derive`'s joined concept list, not what
happened.**

`said()` reads `Interaction::prompt_text`. Nothing in Mooshik ever writes a
person's own words there. The three things that open an interaction in this
product are:

* `Memory::derive` / `derive_for_ingest` — `prompt_text = concepts.join("; ")`
  (lambo `memory.rs:1596`, and again at `1751` for the async path),
* `Memory::record_action` — `prompt_text = action.action` (`memory.rs:1800`),
* `Memory::demote` — `prompt_text = None` by design.

Mooshik's chat loop never records the user's turn at all (`src/companion/chat.rs`
opens no `Memory`; the only write path is the model calling `lambo_derive`
through `src/tools/mod.rs:344`).

Executed, on a real 100-column pty, against the seeded sqlite graph — this is
the Today panel as `mooshik tui` actually draws it:

```
|  16:25  The Windpipe ring never holds more   |
|         than 512 in-flight messages;         |
|         Overflow writers block instead of    |
|         dropping messages                    |
|  16:25  Ingested file                        |
|         document:git:/Users/narayan/work/la  |
|         mbo#4c6fc93                          |
```

The mid-sentence `;` is `join("; ")` reaching the screen. Half the day's log is
`Ingested file <absolute path>`, wrapped mid-token.

`Entry`'s own documentation says "One line of a day: a time, and what
happened." A semicolon-joined list of extracted concepts is not what happened;
it is what was extracted, restated. The module header is explicit that this is
the line it will not cross — "the gutter is a summary written for it, not a
truncated log … An empty field is a true statement; a mechanically truncated
log dressed as a written summary is not" — and then fills `entries` with a
mechanically **joined** one. `highlights`, `notes`, `mood` and `because` were
all left empty on that principle. `entries` should have been held to it.

*Remediation.* Two defensible options, and the milestone should pick one out
loud rather than inherit the current answer by accident:

* **Leave `entries` empty on the live path** for the same reason the other four
  are empty, and let M12c's reflect pass write a day's lines. This is the
  consistent choice and it costs the Today panel its only content.
* **Keep it, but say what it is.** Only quote interactions whose `prompt_text`
  is not a derive echo — i.e. drop any interaction whose prompt equals the
  joined contents of the concepts it derived, and drop `record_action` turns
  the same way R1-1 drops their concepts. On today's corpus that empties the
  panel, which is the honest answer, and it fills correctly the moment
  something writes a real turn.

Either way, `Ingested file /Users/…` must not reach a `Day::entry`.

### P2

**M12a-R1-3 — The lock order that prevents a hard deadlock is a comment, and
nothing else.**

`of_memory`'s docstring says `stats` is read before the graph guard because
`Memory::stats` takes the graph lock itself. That is true — proved above by a
reversed-order thread that never completed. It is also protected by nothing: no
test, no type, no assertion. The natural simplification —

```rust
let graph = memory.graph().read();
of_graph(&graph, &memory.stats(), now)
```

— compiles, reads better, passes the whole suite on an idle graph, and hangs
`mooshik tui` forever the first time the flush or canonization task queues a
write between the two acquires. No error, no timeout, no diagnostic: the pane
just stops. M12b's 250 ms tick makes the collision window a hundred times more
likely than it is today.

*Remediation.* A regression test with a watchdog: take the guard, spawn a
writer, call `stats()` on the same thread, and fail if it has not returned
inside a second (the probe used for this review is exactly that shape and
already demonstrates the hang). Cheaper alternative if a deadlocking test is
unwelcome in CI: have `of_memory` take `stats` by value from a caller that
cannot hold a guard, so the ordering is a signature rather than a sentence.

---

**M12a-R1-4 — "closed … whichever way the loop ended" is false for every
signal, and the lease now makes that cost something.**

The module header claims the session is closed after the terminal is put back
"whichever way the loop ended". Executed on a sized pty against a real session:

| How it ended | Alt screen left | Lease released |
| --- | --- | --- |
| `Esc` | yes | yes |
| draw refused (no tty) | n/a | yes |
| `SIGTERM` | **no** | **no** |
| `SIGHUP` (terminal window closed) | **no** | **no** |

After SIGHUP, `mooshik stats` in the next shell exits **2** with "session
mooshik is already held by another writer (…#52769) … and is still refreshing
it" — for a pid that no longer exists. Measured: the lockout lasts the full
`LEASE_TTL` (45 s in lambo `store/lease.rs:92`); at ~55 s the lease lapses and
`stats` succeeds. Lambo's `Drop for Memory` says so in as many words: it aborts
the heartbeat and deliberately does **not** release the lease, because a handle
dropped without a clean close is the crash-shaped path.

A panic inside `draw` is the same shape — the panic hook restores the terminal,
the unwind drops `Memory`, and `close()` never runs. So the implement report's
claim that "the lease is released even when a draw fails / panics" is right
about *fails* and wrong about *panics*.

Forty-five seconds is bounded, and for a crash that is the correct design. What
makes it a finding is that **closing the terminal window is not a crash** — it
is the most ordinary way anyone ends a full-screen ambient pane, and M11 had no
lease to leave behind. The clean exits (`Esc`, `Ctrl-C`, `Ctrl-Q`) all reach
`close()`; nothing outside the key loop does.

*Remediation.* Install a SIGTERM/SIGHUP handler for the length of the live
session that restores the terminal and closes the session — `tokio`'s `signal`
feature is already enabled in `Cargo.toml`. Failing that, correct the module
header: it currently promises something the code does not do, and this repo's
diary is the place those promises are checked.

---

**M12a-R1-5 — The +05:45 pin cannot see a DST regression. Proved by mutation.**

`view_tests.rs` opens by defending its clock: "+05:45 on purpose: it is not a
whole number of hours, so a day boundary computed by rounding the timestamp
instead of converting the calendar date comes out wrong by more than it could
by luck." That is true of *rounding*. It is not true of the other way to get
this wrong, and every test in the file is a `DateTime<FixedOffset>`, which by
construction cannot tell a zone from an offset.

Mutation applied to `of_graph` — one line, of the kind a later reader writes
while "simplifying away the generic":

```rust
let now = now.with_timezone(&now.offset().fix());
```

**`cargo test --lib` → 518 passed, 0 failed.** Every view test, every day-
boundary test, and the live-session test all agree with it. The probe then
shows what it costs: at `TZ=America/New_York`, an interaction at
`1 Nov 00:30 EDT` moves off Sun 1 and onto **Sat 31** — the M9 wrong-day fault,
in the one module written to prevent it. Mutation reverted; tree verified clean.

Severity is regression risk, not present behaviour: the code is correct today.
But the whole reason `view.rs` takes the zone as a parameter is so this axis
can be tested, and the suite does not test it.

*Remediation.* One test with a zone that has a transition. `chrono::Local` under
a pinned `TZ` is enough and needs no new dependency (executed here that way);
if CI's environment makes `TZ` unreliable, a hand-rolled `TimeZone` impl with
one transition is a dozen lines and pins it without touching the environment.

---

**M12a-R1-6 — Five thread slots, and an LLM extractor's paraphrases take
them.**

`THREADS = 5`, and nothing deduplicates. Executed: a corpus of three
paraphrases of one fact, each reached twice, produces three of the five
threads:

```
"The ring never holds more than 512 in-flight messages"
"The ring has a maximum capacity of 512 in-flight messages"
"The ring caps at 512 in-flight messages; overflow blocks"
```

These are the exact three strings post-M10 recorded from the live corpus. That
document's conclusion was that the paraphrase residue "costs **canonization** …
and costs recall nothing", and accepted it on the grounds that Mooshik leans on
recall. This panel is neither: it is a display of *recurrence*, which is the
one axis paraphrasing provably destroys. The accepted trade does not cover it,
and the panel inherits the pathology with no defence.

*Remediation.* The measurement is already done — post-M10 established a
paraphrase radius of 0.02 in the embedding space. Collapsing threads whose
concepts sit inside it before taking the top five is the principled fix, and
`Concept::embedding` is on the node. If that is too much for M12a, say in the
module header that the list can show one thought several times and why, so the
next reader does not rediscover it as a bug.

### P3

**M12a-R1-7 — The refusal points the operator at a document that does not
exist.** Lambo's conflict sentence ends "see the single-writer lease note in
`docs/reference/cli.mdx`". `mooshik/docs/` contains exactly one file,
`SPEC.md`. The `tui_cmd` header advertises this sentence as one that "names the
holder and the override"; the override it names is a dead path. Post-M10
already listed "Lambo vocabulary leaks into user-facing strings" as an open
gap — M12a puts it on the primary surface's refusal path. Either rewrite the
detail into Mooshik's own `memory.session_conflict` template or add the note.

**M12a-R1-8 — The one live-path test never touches the store the product runs
on.** `a_live_session_fills_the_workspace_the_screens_draw` sets
`StoreKind::Memory`, so nothing serializes and nothing reloads. Post-M10's own
lesson is one sentence long: "A test that runs only against SQLite or the
in-memory store cannot catch an adapter-specific decode bug — and the product
store is the one that was broken." The round-trip does work (verified by hand
through sqlite for this review), which is why this is P3 and not higher.
`StoreKind::Sqlite` with a `tempfile` path costs one line and closes it.

**M12a-R1-9 — `of_graph` holds the graph read lock for its whole pass and
allocates one `Entry` per turn of the week.** Measured (debug build): 1 000
interactions / 400 concepts → **4.6 ms**; 4 000 / 1 500 → **19.3 ms**, steady
state, with **4 000 `Entry` values** (two `String`s each) built for panels that
draw about twenty rows. Fine as a one-shot. Flagged for **M12b**, not for
remediation here: at a 250 ms tick that is a ~20 ms window per tick in which
the flush and canonization tasks cannot take the write lock, on a graph a
fifth the size of a year's use. The cheap fix when M12b lands is to cap
`log[index]` at what any panel can draw, and to build the view from a cloned
slice rather than under the guard.

**M12a-R1-10 — `week_dates`'s saturating fallback can produce seven identical
days, and `today_index` then matches all of them.** `checked_sub_days(…)
.unwrap_or(today)` silently collapses the week when the subtraction underflows.
Executed at `NaiveDate::MIN`: label `"1-1 January"`, seven `Thu` heads, seven
`day_of_month == "1"`, and `screen::today::today_index` — which finds today by
matching `day_of_month` — matches all seven and picks column 0 while the ribbon
brightens column 6. Unreachable from any real clock (seven consecutive dates
always have seven distinct days of the month), so this is a robustness note,
not a live defect. Worth recording because `today_index`'s correctness rests on
a distinctness invariant that `week_dates` is the sole guarantor of and does not
state.

**M12a-R1-11 — A flat week draws seven full bars: the floor has no ceiling.**
Executed: `[1,1,1,1,1,1,1]` → `███████`, identical to `[9,9,9,9,9,9,9]`.
`bar_level` argues carefully that a day which happened must never draw the
empty day's glyph, and lifts any activity to step 2. The same argument runs the
other way and is not made: a week of single turns draws the loudest ribbon the
app can draw. The live pty run showed it on real data — three ordinary
four-turn days rendered `▁▁▁█▁██`. Relative scaling is a defensible design
call; the asymmetry between a floor that is defended in eight lines of comment
and a ceiling that is not mentioned is what makes this worth a line.

**M12a-R1-12 — The calendar tables are read through a panicking dynamic key
with no completeness pin.** `weekday()` and `month()` build their key with
`format!("{table}.{}", …)` and hand it to `text::get`, which **panics** on a
missing key — inside the draw path. `en.toml` is complete today and
`every_leaf_is_a_nonempty_string` checks the leaves it has, but nothing checks
that all seven weekdays and all twelve months are *present*. No test fetches
months 2–7 or 10–12 (the suite lives in August, September and January). A
locale file with one gap ships a TUI that panics in that month. One loop over
`1..=12` and `1..=7` pins it.

**M12a-R1-13 — `days()` re-derives `about_time` three times over, past the map
that exists to stop it.** `placements`'s docstring is explicit: "Once, because
three separate readers want the answer … and a graph with an interaction per
turn of a long session is not a thing to walk three times a tick." `days()`
then looks up `placed` for the day index but calls `interaction.about_time()`
again in the sort comparator (twice per comparison, so `O(n log n)` times) and
a third time for the clock string. Cheap today; it also means the `Placed.at`
field has exactly one reader (`threads`) while the value is recomputed
everywhere else, which is the shape that lets the two drift apart later.

**M12a-R1-14 — `Health::short_scope` is still the long form, and `scope` still
omits the half the graph could now answer.** `model.rs` documents these as two
*written* forms — `"214 things remembered, back to 21 August"` and
`"214 remembered"` — and says in as many words why the short one is not a
truncation. `health()` emits `tui.scope_live` for both, so the 80-column slot
gets the wide string. Inherited unchanged from M11's `from_stats`, so not a
regression — but M12a is the commit that gave this module a graph, and "back to
21 August" is now one `min(about_time)` away. Either fill both fields properly
or amend the model's documentation to match what is emitted.

## Mutation-tested pins

| Mutation | Result |
| --- | --- |
| `of_graph` collapses the reader's zone to its offset at draw time (`now.offset().fix()`) | **survives** — 518/518 pass; probe shows `1 Nov 00:30 EDT` moving to Oct 31 (P2, R1-5) |
| `of_memory` takes the graph guard before `stats()` | **not caught by any test** — deadlocks under a queued writer, proved by watchdog; no pin exists (P2, R1-3) |

Both transient; `src/memory/view.rs` restored from a byte copy and the tree
verified clean (`git status --porcelain` empty).

## What was executed vs. only read

**Executed.** `cargo test --locked` (518 lib + 1 integration, 0 failed);
`cargo clippy --all-targets --all-features`; `cargo fmt --check`; the
1000-line cap by hand. A nine-case probe harness (`tests/m12a_probe.rs`,
written, run and deleted) covering bar levels at seven shapes, week labels
across month/year/leap boundaries, the DST fall-back day under
`TZ=America/New_York` and `TZ=Asia/Kathmandu`, the UTC/local seam at five
offsets, thread edges (both-supports-outside-the-week, same-day supports,
paraphrase crowding), historical placement, `NaiveDate::MIN`, and scale at
1 000 and 4 000 interactions. The two mutations above. A scratch
`MOOSHIK_HOME` on sqlite, seeded through `mooshik serve`'s real MCP stdio
surface (`lambo_derive` with `parent_of`, `lambo_record_action`, both with
`event_time`), then rendered three ways: `of_memory` over the reopened store,
an 80×24 pty and a 100/120-column pty driven by a Python `pty.fork` harness.
The lease conflict against a live `mooshik serve` holder (exit 2 verified
without a pipe in the way), the SIGTERM and SIGHUP paths, and the 45-second
TTL lapse.

**Read, not executed.** The Postgres adapter's `prompt_text`/`event_time`
round-trip (read from `store/pg/mod.rs`; sqlite was executed). The panic-in-
`draw` path (traced through `Drop for Memory`; the SIGTERM result establishes
the same conclusion by a route that could be executed). The Week screen's
header at 80 columns — `day_header` was exercised at 120 by the repo's own
tests and by mine, and the narrow layout draws no thread header at all.

## Outside this milestone, noted in passing

`mooshik init` created `graph.db` with mode `0644` while every other file it
writes is `0600`. Everything the user has ever remembered is world-readable on
a shared machine. This is lambo's sqlite creation, not M12a's change — but
M12a is the commit that made that file the thing the primary surface opens, so
it belongs in the record. Also: `mooshik init` against the default
`kind = "postgres"` config hung past two minutes with no output on macOS
before I switched the store to sqlite; not investigated.
