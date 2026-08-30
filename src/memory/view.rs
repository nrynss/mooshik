//! The live workspace: the graph, said the way the screens want to hear it.
//!
//! M11 built the surface and left it with no source — "the panels are empty,
//! because the data the artboards show has no source behind Mooshik yet". This
//! module is that source. It is a **pure function of a graph and a clock**:
//! [`of_graph`] takes a borrowed [`Graph`], the session's [`MemoryStats`] and the
//! instant to draw, and returns the same [`Workspace`] the screens already read,
//! so nothing under `tui::screen` learns that a database exists.
//!
//! ## Every placement resolves through `about_time`
//!
//! An [`Interaction`] carries two clocks: `created_at`, when Lambo flushed it,
//! and `event_time`, the instant the turn is *about*. M9 measured what happens
//! when a reader picks the wrong one — a decade of commit dates collapsed into
//! one afternoon and canonization promoted nothing. So every day bucket, every
//! thread mark and every trickle line here resolves through
//! [`Interaction::about_time`], never a bare field.
//!
//! **A [`Concept`] has neither of those.** It has no `event_time` at all and its
//! `created_at` is a flush stamp, so a concept's about-time is its *origin
//! interaction's* — which is exactly what the M8 ingester means when it stamps a
//! document's date onto the turn that read it. [`about`] is the only place that
//! rule is written, and `a_concept_is_placed_by_the_turn_that_produced_it` is
//! what holds it: a concept flushed a second ago from a turn about 2015 belongs
//! to 2015, and must not appear in this week.
//!
//! ## What this milestone does not write
//!
//! Three fields of [`Day`] and one of [`Thread`] are **prose** — a day's gutter
//! summary and its trailing notes, a day's mood, and why a thread is where it
//! is. [`Day::highlights`]'s own documentation is explicit that it is "a summary
//! written for it, not a truncated log", and [`Justification`] "holds prose
//! rather than a figure". There is nothing in a graph that writes an English
//! sentence, so this module leaves all four empty and M12c's reflect pass fills
//! them. Every screen already draws their absence: a day column with no
//! highlights draws its frame and its date, `Day::detail_entries` falls back the
//! other way, and both thread panels test `Justification::is_empty` before
//! spending a row on it. An empty field is a true statement; a mechanically
//! truncated log dressed as a written summary is not.
//!
//! Weather has no source of any kind and is [`None`] for the same reason, which
//! the model already calls for: "the line is then omitted rather than filled
//! with a placeholder".
//!
//! ## What the panels refuse to draw
//!
//! A graph is not a list of thoughts. Two of the things in it are the engine's
//! own record of its work, and on the only corpus this product can produce they
//! outrank everything the user actually thought:
//!
//! * The bootstrap ingester names every document it reads `document:<source>`
//!   and hangs the facts it extracted off that anchor, so the anchor gains a
//!   `Derives` edge from every turn that touched the document — which is exactly
//!   the count [`threads`] ranks by, and is why post-M10 measured that the only
//!   nodes ever to reach Venerable were `document:file:…` resources. It is also
//!   an absolute path out of the reader's home directory, on a pane whose whole
//!   premise is that it is left open beside their work.
//! * `record_action` opens a concept for the action itself — "Ingested file
//!   document:git:…" — which is bookkeeping about a write, not a thing
//!   remembered.
//!
//! [`bookkeeping`] is the one seam that says so, and both panels pass through
//! it. Nothing else in this module filters by content.
//!
//! The same argument decides [`Day::entries`]. `Interaction::prompt_text` is the
//! only field in the graph that carries words, and **nothing in Mooshik writes a
//! person's own words into it**: `derive` fills it with the concepts it is about
//! to write, joined with `"; "`, and `record_action` with the action string it
//! is about to write as a concept. Quoting either back is the engine's echo
//! dressed as a day's log — the line [`Day::highlights`] refuses to cross for
//! prose, crossed for the one field that was filled. So [`said`] drops a turn
//! whose words are the concepts it wrote, which empties the log on today's
//! corpus, and fills it the moment something records a real turn.
//!
//! ## Named `view`, not `snapshot`
//!
//! [`Graph::snapshot`] already exists and means persistence — the whole graph,
//! serialized for the store. This is a *view*: seven days of it, ordered and
//! formatted for one screen size of one terminal, and thrown away on the next
//! tick. Two words for two things.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, Days, NaiveDate, TimeZone, Timelike, Utc};
use lambo::{Concept, EdgeType, Graph, Interaction, Memory, MemoryStats, NodeId};

use crate::{
    text,
    tui::{
        model::{
            Conversation, Day, Entry, Health, Justification, Load, Stamp, Thread, Tone, Trickle,
            Week, Workspace,
        },
        widget::marks,
    },
};

/// Days on screen — the ribbon's seven bars, a thread's seven marks, and the
/// week screen's seven columns are all this number.
const WEEK: usize = marks::WEEK;

/// How many threads and trickle lines the view carries: as many as the
/// artboards draw. `1a` lists five of each and `1b` five threads, and the panels
/// window or clip whatever they are given, so more would only be allocated every
/// tick and thrown away below the rule.
const THREADS: usize = 5;
const TRICKLE: usize = 5;

/// The fewest turns that count as *coming back* to something.
///
/// The panel is called "What keeps coming back", and a concept derived once has
/// not come back — it has arrived. Lambo guarantees a `Derives` edge from every
/// interaction that writes a concept, creating it the first time and reinforcing
/// it when the *same* turn repeats itself, so the number of distinct
/// interactions on the other end of those edges is exactly how many separate
/// times the user returned to the thought. Two is the smallest number that is a
/// return.
///
/// A graph where nothing has recurred yet shows an empty panel, which is a true
/// statement about that graph. `a_thought_reached_once_is_not_a_thread` holds
/// the floor from below and `an_old_thought_is_not_this_weeks_thread` from the
/// other side.
const RETURNS: usize = 2;

/// How many recurrence clusters are carried while the list is being built.
///
/// Wider than [`THREADS`], because the fold is what decides the order: a thought
/// whose paraphrases each rank below the panel outranks a single stronger
/// thought once they are counted together, and a pool the width of the panel
/// would have dropped every one of them before they could be folded. Bounded
/// rather than unbounded so the fold stays a fixed number of comparisons per
/// concept on a graph whose size nothing here controls.
const POOL: usize = 4 * THREADS;

/// The most lines of one day's log this view carries.
///
/// A day's log has no scroll — `aside::entries` stops at the panel's last
/// interior row and no key reaches past it — so every line beyond the tallest
/// interior any terminal can draw is built and thrown away. The earliest of the
/// day are kept, because that is the end the panels draw from.
const LOG: usize = 256;

/// The prefix Mooshik's own bootstrap ingester puts on a document anchor. See
/// this module's header for why an anchor must not reach a panel.
const PROVENANCE: &str = "document:";

/// What [`Memory::derive`] joins a turn's concepts with when it fills
/// `prompt_text`.
const JOIN: &str = "; ";

/// How close two thoughts have to sit, in the embedding space, to take one row
/// between them.
///
/// Measured rather than chosen. Post-M10 sampled forty concepts of the clean
/// graph through pgvector: median nearest-neighbour distance 0.031 against a
/// median distance-to-everything of 0.353, and genuine paraphrases inside 0.02.
/// That is the radius, and it is the number this uses.
///
/// **Display only.** Folding two rows into one is a statement about a panel with
/// five slots, not about the graph: nothing is merged, nothing is promoted, and
/// the next tick asks the same question again. Consolidating the nodes
/// themselves is a write, and M12c's.
const PARAPHRASE: f32 = 0.02;

/// The workspace for an open [`Memory`], as of `now`.
///
/// **Two acquires, in this order and no other.** [`Memory::stats`] takes the
/// graph lock itself, and `parking_lot`'s read lock is not recursion-safe: a
/// writer queued between the two deadlocks a thread that already holds one
/// reader, with no error, no timeout and no diagnostic — the pane simply stops.
/// [`of_graph`] takes the figures ahead of the graph so that the one-expression
/// form of this call, which is what a later reader writes while simplifying, is
/// the safe one: Rust evaluates arguments left to right, so the figures are read
/// before the guard exists. `the_figures_are_read_before_the_graph_guard` pins
/// the order, and pins it by reading this body rather than by executing the
/// fault: a test of the reversed order cannot fail, only fail to return.
pub fn of_memory<Tz: TimeZone>(memory: &Memory, now: DateTime<Tz>) -> Workspace {
    of_graph(&memory.stats(), &memory.graph().read(), now)
}

/// The workspace for a graph and a clock, with nothing else behind it.
///
/// `now` carries its own time zone, and every stored instant is resolved into
/// that zone before it is placed on a calendar day. The zone is a parameter
/// rather than [`chrono::Local`] read inside so the day-boundary cases can be
/// tested at a pinned offset — the suite must not answer differently on a
/// developer's laptop than in CI, which runs in UTC. It is a full [`TimeZone`]
/// and not an offset because those are different things on two days a year;
/// `a_zone_is_not_an_offset_across_a_daylight_saving_change` is what holds the
/// distinction.
pub fn of_graph<Tz: TimeZone>(stats: &MemoryStats, graph: &Graph, now: DateTime<Tz>) -> Workspace {
    let zone = now.timezone();
    let dates = week_dates(&now);
    let placed = placements(graph, &zone, &dates);
    // Both collected once, in one pass each, because every panel below asks the
    // same two questions of every node it considers: is this the engine's own
    // record-keeping, and are these words the concepts a turn wrote.
    let actions = action_nodes(graph);
    let contents = concept_contents(graph);
    let days = days(graph, &zone, &dates, &placed, &contents);
    // Today is the last of the seven, and it is the same `Day` the ribbon's last
    // column draws — `screen::today::today_index` finds it by matching
    // `day_of_month`, so the two must be one day formatted once.
    let today = days[WEEK - 1].clone();
    Workspace {
        person: text::get("tui.person_unknown").to_owned(),
        now: stamp(&now),
        today,
        week: Week {
            label: week_label(dates[0], dates[WEEK - 1]),
            day_heads: dates.iter().map(|date| weekday(date, true)).collect(),
            days,
            // The detail pane opens on today. `1b` opens on the hard day, which
            // is a fact about the artboard's Wednesday and not something a graph
            // can nominate.
            selected: WEEK - 1,
        },
        threads: threads(graph, &placed, &actions),
        trickle: trickle(graph, &placed, &actions),
        // Not fed from the graph. The conversation is the chat loop's, and until
        // it can be driven from a redraw loop the panel stays as M11 left it.
        conversation: Conversation::default(),
        health: health(stats, earliest(&placed, &zone)),
    }
}

/// The status bar: one word for the session's state, and how much is behind it.
///
/// Two words at most, per `1i`: "One mark, one word: reachable, saved, keeping
/// up. Never a sentence."
///
/// `since` is the day the earliest turn in the graph is about — the far end of
/// what this session remembers, on the reader's own calendar. The model
/// documents these as two *written* forms, "214 things remembered, back to 21
/// August" beside "214 remembered", and says why the short one is not a
/// truncation of the long one; both are written here. A graph with nothing in it
/// has no far end, and then the long form is the short one plus its noun.
pub(crate) fn health(stats: &MemoryStats, since: Option<NaiveDate>) -> Health {
    let (state, well) = if stats.degraded {
        (text::get("tui.health_degraded"), false)
    } else if stats.log_depth > 0 {
        (text::get("tui.health_catching_up"), false)
    } else {
        (text::get("tui.health_keeping_up"), true)
    };
    let count = stats.node_count.to_string();
    let scope = match since {
        Some(date) => text::get("tui.scope_since")
            .replace("{count}", &count)
            .replace("{date}", &day_month(date)),
        None => text::get("tui.scope_live").replace("{count}", &count),
    };
    Health {
        state: state.to_owned(),
        scope,
        short_scope: text::get("tui.scope_short").replace("{count}", &count),
        well,
    }
}

/// The day the earliest turn in the graph is about, in the reader's zone.
///
/// The about-time and not the flush stamp, for the reason the whole module
/// resolves through [`Interaction::about_time`]: a bootstrap flushed this
/// afternoon is about a decade ago, and "back to this afternoon" is the sentence
/// M9 measured the cost of.
fn earliest<Tz: TimeZone>(placed: &HashMap<NodeId, Placed>, zone: &Tz) -> Option<NaiveDate> {
    placed
        .values()
        .map(|place| place.at)
        .min()
        .map(|at| at.with_timezone(zone).date_naive())
}

/// Where one interaction sits: the instant it is about, and which of the week's
/// days that lands on in the reader's own zone.
#[derive(Debug, Clone, Copy)]
struct Placed {
    at: DateTime<Utc>,
    day: Option<usize>,
}

/// Every interaction in the graph, placed once.
///
/// Once, because four separate readers want the answer — the day logs, a
/// thread's marks, the trickle's freshness and the far end the status bar names
/// — and a graph with an interaction per turn of a long session is not a thing
/// to walk four times a tick.
fn placements<Tz: TimeZone>(
    graph: &Graph,
    zone: &Tz,
    dates: &[NaiveDate; WEEK],
) -> HashMap<NodeId, Placed> {
    graph
        .interactions()
        .map(|interaction| {
            let at = interaction.about_time();
            (
                interaction.id,
                Placed {
                    at,
                    day: day_index(zone, dates, at),
                },
            )
        })
        .collect()
}

/// The instant a concept is about: its origin interaction's, because a concept
/// carries no event time of its own and its `created_at` is a flush stamp.
///
/// `None` when the graph does not hold that interaction. Lambo refuses to insert
/// a concept whose origin is not an interaction in the same graph, so this is
/// unreachable through the write path — but a partial load has no such promise,
/// and inventing a time for an unplaceable fact is how a bootstrap ends up
/// looking like an afternoon.
fn about(placed: &HashMap<NodeId, Placed>, concept: &Concept) -> Option<Placed> {
    placed.get(&concept.origin_interaction).copied()
}

/// The seven calendar dates on screen, oldest first, ending on the day `now`
/// falls in.
///
/// **The week trails today; it is not a calendar week.** The ribbon is read as
/// the shape of the last seven days, and `Thread::days` marks the days a thought
/// came up on — both are answers about what has happened, and a Friday-anchored
/// week asked on a Saturday would spend five of its seven columns on days that
/// have not happened yet while hiding the five that just did. The design's own
/// week is Friday-first because the design's own day is a Thursday; what
/// generalizes is "today is the last column", which `screen::today::today_index`
/// already says in as many words.
///
/// **The seven dates are distinct, and something depends on that.**
/// `screen::today::today_index` finds today by matching `day_of_month`, so a
/// week holding one date twice brightens one ribbon column while the panel
/// describes another. Seven consecutive dates are distinct by arithmetic; the
/// only way to lose that is a subtraction that underflows and falls back to
/// today, which is why the anchor is lifted to the first date with six days
/// behind it rather than left to fold the whole week onto one day.
fn week_dates<Tz: TimeZone>(now: &DateTime<Tz>) -> [NaiveDate; WEEK] {
    let span = u64::try_from(WEEK - 1).unwrap_or(0);
    let first_drawable = NaiveDate::MIN
        .checked_add_days(Days::new(span))
        .unwrap_or(NaiveDate::MIN);
    let today = now.date_naive().max(first_drawable);
    std::array::from_fn(|index| {
        let back = u64::try_from(WEEK - 1 - index).unwrap_or(0);
        today.checked_sub_days(Days::new(back)).unwrap_or(today)
    })
}

/// Which of the week's days a UTC instant lands on, in the reader's zone.
///
/// The conversion is to a local *date*, not an offset arithmetic on the
/// timestamp: a day boundary is a property of the calendar the reader is
/// looking at, and on the two days a year that are 23 or 25 hours long, nothing
/// else gets it right.
fn day_index<Tz: TimeZone>(
    zone: &Tz,
    dates: &[NaiveDate; WEEK],
    at: DateTime<Utc>,
) -> Option<usize> {
    let local = at.with_timezone(zone).date_naive();
    dates.iter().position(|date| *date == local)
}

/// The week's seven days, each with its log and its bar.
fn days<Tz: TimeZone>(
    graph: &Graph,
    zone: &Tz,
    dates: &[NaiveDate; WEEK],
    placed: &HashMap<NodeId, Placed>,
    contents: &HashSet<&str>,
) -> Vec<Day> {
    // The placement travels with the turn rather than being looked up again:
    // `placements` exists because three readers want the same answer, and a
    // comparator that recomputes `about_time` twice per comparison is the shape
    // that lets `Placed::at` and the value actually drawn drift apart.
    let mut logs: [Vec<(Placed, &Interaction)>; WEEK] = std::array::from_fn(|_| Vec::new());
    let mut counts = [0usize; WEEK];
    for interaction in graph.interactions() {
        let Some(place) = placed.get(&interaction.id).copied() else {
            continue;
        };
        let Some(index) = place.day else {
            continue;
        };
        // The bar counts every turn, whether or not it left something to quote:
        // it measures how full the day was, not how much of it is printable.
        counts[index] = counts[index].saturating_add(1);
        if said(contents, interaction).is_some() {
            logs[index].push((place, interaction));
        }
    }
    let busiest = counts.iter().copied().max().unwrap_or(0);

    dates
        .iter()
        .enumerate()
        .map(|(index, date)| {
            let mut log = std::mem::take(&mut logs[index]);
            // By about-time, then by id: two turns stamped with the same instant
            // are ordinary in a bulk ingest, and a log that reshuffles them
            // between two ticks of the same graph would be its own bug.
            log.sort_by(|(left, left_turn), (right, right_turn)| {
                left.at
                    .cmp(&right.at)
                    .then_with(|| left_turn.id.0.cmp(&right_turn.id.0))
            });
            log.truncate(LOG);
            Day {
                short_label: day_head(*date),
                long_label: long_date(*date),
                day_of_month: date.day().to_string(),
                // No source, on either. See this module's header.
                weather: None,
                mood: None,
                load: Load::new(bar_level(counts[index], busiest), Tone::Plain),
                entries: log
                    .into_iter()
                    .map(|(place, interaction)| {
                        Entry::at(
                            &clock(&place.at.with_timezone(zone)),
                            said(contents, interaction).unwrap_or_default(),
                        )
                    })
                    .collect(),
                // M12c writes both. See this module's header.
                highlights: Vec::new(),
                notes: String::new(),
            }
        })
        .collect()
}

/// What a turn has to show for itself, or nothing.
///
/// An interaction with no prompt — `demote` opens one for a chunk that
/// overflowed rather than something that was said — has no line to contribute,
/// and an [`Entry`] with empty text is a blank row with a timestamp beside it.
///
/// Neither does a turn whose words are the concepts it wrote. `derive` fills
/// `prompt_text` with its concepts joined by [`JOIN`] and `record_action` with
/// the action string it writes as a concept of its own, so both are caught by
/// one test: every piece of the prompt is the content of a concept in this
/// graph. The whole prompt is tried before the split, because one concept may
/// itself contain the separator.
///
/// The test is over the graph's contents rather than this turn's own, because a
/// turn that re-derives an existing thought creates no concept — the reinforced
/// concept's origin is the first turn that reached it, not this one. The cost is
/// one over-approximation: a person whose sentence is *exactly* a concept in the
/// graph is read as an echo. Nothing in this product records a person's sentence
/// yet, and when something does, an extracted fact is not the sentence it came
/// from.
fn said<'a>(contents: &HashSet<&str>, interaction: &'a Interaction) -> Option<&'a str> {
    let said = interaction
        .prompt_text
        .as_deref()
        .map(str::trim)
        .filter(|said| !said.is_empty())?;
    if contents.contains(said) || said.split(JOIN).all(|piece| contents.contains(piece)) {
        return None;
    }
    Some(said)
}

/// Every concept's content, once, so [`said`] can ask whether a turn is quoting
/// the graph back at itself without walking it per turn.
fn concept_contents(graph: &Graph) -> HashSet<&str> {
    graph
        .concepts()
        .map(|concept| concept.content.as_str())
        .collect()
}

/// The concepts `record_action` opened for an action, collected in one pass over
/// the edges.
///
/// `Causal` and `Dependency` edges have exactly one writer in Lambo —
/// `graph::action::record_action`, which plans them from the action node to what
/// the action produces, modifies and depends on — so a concept at the source of
/// one is an action node and not a thought. `ConceptType` cannot answer this: an
/// action node is `Resource`, a document anchor is `Entity`, and concepts the
/// user meant are both.
///
/// An action recorded with no targets has no such edge and reads as an ordinary
/// thought. That is the honest limit of this: the bookkeeping this product
/// actually writes always names what it produced.
fn action_nodes(graph: &Graph) -> HashSet<NodeId> {
    graph
        .edges()
        .filter(|edge| matches!(edge.edge_type, EdgeType::Causal | EdgeType::Dependency))
        .map(|edge| edge.source)
        .collect()
}

/// Whether a concept is the engine's record of its own work rather than
/// something the user thought. See this module's header.
fn bookkeeping(actions: &HashSet<NodeId>, concept: &Concept) -> bool {
    concept.content.starts_with(PROVENANCE) || actions.contains(&concept.id)
}

/// How tall a day's bar is, as a share of the busiest day in the same week.
///
/// Relative, not absolute: eight steps against a fixed number of turns a day
/// would flatten every quiet week to the baseline and saturate every loud one,
/// and the ribbon is read as a shape. The busiest day of the week on screen
/// reaches the tallest glyph and an empty day gets the shortest — never nothing,
/// because a zero would leave a hole in the ribbon (see [`Load::new`]).
///
/// **A day that happened never draws the empty day's bar.** The share alone
/// rounds a single turn beside a thirty-turn Monday down to zero, and the
/// baseline glyph is what an empty Sunday already draws — so the ribbon said
/// "nothing happened" about a day that did, which is the one thing this row is
/// asked. Any activity is lifted to the second step, and the seven steps above
/// the baseline carry the shape from there.
///
/// **A week with no busiest day draws seven full bars, and that is the same
/// argument from the other end.** Seven single-turn days are each 100% of their
/// own week, so `▁▁▁█▁██` and `███████` differ in shape and not in loudness —
/// the height is a share and carries no absolute meaning, which is stated above
/// and is what keeps a quiet week from flattening onto the baseline. Capping a
/// flat week at a middle step would put an absolute judgement back into exactly
/// one case and make `[9,9,9,9,9,9,9]` and `[9,9,9,9,9,9,8]` two rows apart.
/// `a_flat_week_is_drawn_flat_at_the_top_of_its_own_scale` pins the answer so it
/// is a decision rather than a side effect.
fn bar_level(count: usize, busiest: usize) -> u8 {
    if count == 0 || busiest == 0 {
        return 1;
    }
    let steps = Load::BARS.len().saturating_sub(1);
    let scaled = steps
        .saturating_mul(count)
        .saturating_add(busiest / 2)
        .saturating_div(busiest)
        .max(1);
    u8::try_from(scaled.saturating_add(1)).unwrap_or(u8::MAX)
}

/// What a concept is worth as a thread: which turns reached it, and which of the
/// week's days those were.
struct Recurrence<'a> {
    concept: &'a Concept,
    /// The turns carrying a `Derives` edge into the concept, kept rather than
    /// counted because folding two paraphrases has to union them: a turn that
    /// derived both said one thing, not two.
    supports: Vec<NodeId>,
    days: [bool; WEEK],
    latest: DateTime<Utc>,
}

/// "What keeps coming back", strongest first.
///
/// Strength is the count of distinct interactions carrying a `Derives` edge into
/// the concept — how often the user came back to the thought. **Not the
/// canonization tier**: a tier is Lambo's judgement about evidence and this list
/// is the user's own history, which is the whole of `1i`'s argument for it. The
/// count never reaches the screen either; the position in this list is the
/// encoding, and [`Strength::from_rank`](crate::tui::theme::Strength::from_rank)
/// is what draws it.
///
/// A thread must also have come up **inside the week on screen**, because the
/// panel draws its seven marks under the seven day columns: a thought last
/// touched in March would arrive with every mark absent, which reads as "this
/// never comes up" beside a claim that it is what keeps coming back.
///
/// **One thought takes one row.** An LLM extractor does not repeat itself: the
/// same fact met three times enters the graph as three concepts, and post-M10
/// measured the consequence — recurrence spreads across the copies instead of
/// accumulating on one, and three of these five slots go to one thought said
/// three ways. So the strongest candidate of a cluster keeps the row and the
/// rest are folded into it, supports unioned, marks merged. Same thought by
/// [`one_thought`]: Lambo's own canonical key, or inside the measured
/// [`PARAPHRASE`] radius. A copy with no vector yet — writes acknowledge before
/// the embedder runs — is only caught by the key, and still takes its own row.
fn threads(
    graph: &Graph,
    placed: &HashMap<NodeId, Placed>,
    actions: &HashSet<NodeId>,
) -> Vec<Thread> {
    let mut found: Vec<Recurrence<'_>> = graph
        .concepts()
        .filter(|concept| !bookkeeping(actions, concept))
        .filter_map(|concept| recurrence(graph, placed, concept))
        .collect();
    found.sort_by(strongest_first);

    // Strongest first, so the row a cluster keeps is the copy that earned it.
    // A candidate that matches nothing held and finds the pool full is below the
    // cut in every ordering, folded or not, and is dropped; the walk continues so
    // a paraphrase further down still merges into the row it belongs to.
    let mut kept: Vec<Recurrence<'_>> = Vec::new();
    for candidate in found {
        let held = kept
            .iter()
            .position(|held| one_thought(held.concept, candidate.concept));
        match held {
            Some(index) => kept[index].absorb(candidate),
            None if kept.len() < POOL => kept.push(candidate),
            None => {}
        }
    }
    // Folding changes the totals, so the order is settled after it rather than
    // before: a cluster is as strong as what it gathered.
    kept.sort_by(strongest_first);
    kept.truncate(THREADS);

    kept.into_iter()
        .map(|found| Thread {
            summary: found.concept.content.clone(),
            // The one-line label and the reason are both written, not derived —
            // `None` falls back to the summary and an empty reason draws no row.
            short_summary: None,
            days: found.days,
            because: Justification::default(),
            // Only ever drawn under a standing caution, which the live workspace
            // cannot reach: the model's own word for the rest of the time is
            // "empty".
            leaned_on: Vec::new(),
        })
        .collect()
}

/// The list's order: how often it came back, then how much of the week it spans,
/// then how recently — and then two tie-breaks that make the order total, so two
/// equally-returned-to thoughts do not swap places between one tick and the next.
fn strongest_first(left: &Recurrence<'_>, right: &Recurrence<'_>) -> std::cmp::Ordering {
    right
        .returns()
        .cmp(&left.returns())
        .then_with(|| right.day_count().cmp(&left.day_count()))
        .then_with(|| right.latest.cmp(&left.latest))
        .then_with(|| left.concept.content.cmp(&right.concept.content))
        .then_with(|| left.concept.id.0.cmp(&right.concept.id.0))
}

/// Whether two concepts are one thought for the purpose of one row.
///
/// Lambo's own judgement first: a shared canonical key is the engine saying
/// these are the same thing, and it costs a string comparison. The embedding
/// distance is the second leg, because the key cannot see a paraphrase — that is
/// the whole of what post-M10 measured.
fn one_thought(left: &Concept, right: &Concept) -> bool {
    if left.canonical_key == right.canonical_key {
        return true;
    }
    let (Some(here), Some(there)) = (&left.embedding, &right.embedding) else {
        return false;
    };
    cosine_distance(here, there).is_some_and(|distance| distance < PARAPHRASE)
}

/// The distance between two vectors, or `None` when there is no angle to
/// measure: mismatched widths (two embedding contracts in one graph) or a zero
/// vector.
fn cosine_distance(here: &[f32], there: &[f32]) -> Option<f32> {
    if here.len() != there.len() {
        return None;
    }
    let dot: f32 = here.iter().zip(there).map(|(a, b)| a * b).sum();
    let magnitude = norm(here) * norm(there);
    (magnitude > 0.0).then(|| 1.0 - dot / magnitude)
}

fn norm(vector: &[f32]) -> f32 {
    vector.iter().map(|value| value * value).sum::<f32>().sqrt()
}

impl Recurrence<'_> {
    /// How many separate turns reached this thought. Orders the list; never
    /// drawn as a number.
    fn returns(&self) -> usize {
        self.supports.len()
    }

    /// How many of the week's days this thought came up on. Orders the list;
    /// never drawn as a number.
    fn day_count(&self) -> usize {
        self.days.iter().filter(|came_up| **came_up).count()
    }

    /// Take another way of saying the same thing into this row.
    ///
    /// The supports are unioned rather than added: one turn that derived two
    /// paraphrases of a fact is one turn that came back to it, and adding the
    /// counts would let an extractor's habits inflate the ranking it is already
    /// spreading.
    fn absorb(&mut self, other: Recurrence<'_>) {
        for support in other.supports {
            if !self.supports.contains(&support) {
                self.supports.push(support);
            }
        }
        for (day, came_up) in self.days.iter_mut().zip(other.days) {
            *day = *day || came_up;
        }
        self.latest = self.latest.max(other.latest);
    }
}

/// One concept's recurrence, or `None` if it is not something the user keeps
/// coming back to this week.
fn recurrence<'a>(
    graph: &Graph,
    placed: &HashMap<NodeId, Placed>,
    concept: &'a Concept,
) -> Option<Recurrence<'a>> {
    let supports = graph.in_neighbors_typed(concept.id, EdgeType::Derives);
    if supports.len() < RETURNS {
        return None;
    }
    let mut days = [false; WEEK];
    let mut latest: Option<DateTime<Utc>> = None;
    for support in &supports {
        let Some(place) = placed.get(support) else {
            continue;
        };
        if let Some(index) = place.day {
            days[index] = true;
        }
        latest = Some(latest.map_or(place.at, |best| best.max(place.at)));
    }
    if !days.iter().any(|came_up| *came_up) {
        return None;
    }
    Some(Recurrence {
        concept,
        supports,
        days,
        latest: latest?,
    })
}

/// "Just remembered": what the week on screen picked up, freshest first.
///
/// Windowed to the same seven days as everything else. The panel's title is a
/// claim about recency, so a graph nobody has written to since March shows
/// nothing here rather than offering March as news. And filtered through
/// [`bookkeeping`] like the threads are: "Just remembered: Ingested file
/// document:git:/Users/…" is not something anybody remembered.
fn trickle(
    graph: &Graph,
    placed: &HashMap<NodeId, Placed>,
    actions: &HashSet<NodeId>,
) -> Vec<Trickle> {
    let mut fresh: Vec<(DateTime<Utc>, &Concept)> = graph
        .concepts()
        .filter(|concept| !bookkeeping(actions, concept))
        .filter_map(|concept| {
            let place = about(placed, concept)?;
            place.day?;
            Some((place.at, concept))
        })
        .collect();
    fresh.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.content.cmp(&right.1.content))
            .then_with(|| left.1.id.0.cmp(&right.1.id.0))
    });
    fresh
        .into_iter()
        .take(TRICKLE)
        // `returned` is the one blue thing in the app and means a line has come
        // back from another day. Nothing here computes that, so nothing claims it.
        .map(|(_, concept)| Trickle::new(&concept.content))
        .collect()
}

/// The clock in the title bar, at the three renderings the chrome asks for.
fn stamp<Tz: TimeZone>(now: &DateTime<Tz>) -> Stamp {
    let date = now.date_naive();
    Stamp {
        long_date: long_date(date),
        short_date: short_date(date),
        time: clock(now),
    }
}

/// "Thursday 27 August".
fn long_date(date: NaiveDate) -> String {
    text::get("tui.date_long")
        .replace("{weekday}", &weekday(&date, false))
        .replace("{day}", &date.day().to_string())
        .replace("{month}", &month(&date, false))
}

/// "Thu 27 Aug" — what survives at 80 columns.
fn short_date(date: NaiveDate) -> String {
    text::get("tui.date_short")
        .replace("{weekday}", &weekday(&date, true))
        .replace("{day}", &date.day().to_string())
        .replace("{month}", &month(&date, true))
}

/// "21 August" — how far back the status bar says this session goes.
///
/// Its own key rather than [`long_date`] without the weekday: "back to Friday 21
/// August" names a weekday nobody asked about, on the one line of the app that
/// has to survive an 80-column terminal beside the week's own label.
fn day_month(date: NaiveDate) -> String {
    text::get("tui.date_day_month")
        .replace("{day}", &date.day().to_string())
        .replace("{month}", &month(&date, false))
}

/// "Thu 27" — a week column's title.
fn day_head(date: NaiveDate) -> String {
    text::get("tui.day_head")
        .replace("{weekday}", &weekday(&date, true))
        .replace("{day}", &date.day().to_string())
}

/// "14:22", in the zone the instant is already carrying.
fn clock<Tz: TimeZone>(at: &DateTime<Tz>) -> String {
    text::get("tui.clock")
        .replace("{hour}", &format!("{:02}", at.hour()))
        .replace("{minute}", &format!("{:02}", at.minute()))
}

/// "21-27 August", or "29 August - 4 September" when the week crosses a month.
fn week_label(from: NaiveDate, to: NaiveDate) -> String {
    if from.year() == to.year() && from.month() == to.month() {
        return text::get("tui.week_span")
            .replace("{from}", &from.day().to_string())
            .replace("{to}", &to.day().to_string())
            .replace("{month}", &month(&to, false));
    }
    text::get("tui.week_span_across")
        .replace("{from_month}", &month(&from, false))
        .replace("{to_month}", &month(&to, false))
        .replace("{from}", &from.day().to_string())
        .replace("{to}", &to.day().to_string())
}

/// The weekday's name, long or short, from the locale's own table.
fn weekday(date: &NaiveDate, short: bool) -> String {
    let table = if short {
        "tui.weekday_short"
    } else {
        "tui.weekday"
    };
    text::get(&format!("{table}.{}", date.weekday().number_from_monday())).to_owned()
}

/// The month's name, long or short, from the locale's own table.
fn month(date: &NaiveDate, short: bool) -> String {
    let table = if short {
        "tui.month_short"
    } else {
        "tui.month"
    };
    text::get(&format!("{table}.{}", date.month())).to_owned()
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "view_clock_tests.rs"]
mod clock_tests;

#[cfg(test)]
#[path = "view_session_tests.rs"]
mod session_tests;
