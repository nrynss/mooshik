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
//! ## Named `view`, not `snapshot`
//!
//! [`Graph::snapshot`] already exists and means persistence — the whole graph,
//! serialized for the store. This is a *view*: seven days of it, ordered and
//! formatted for one screen size of one terminal, and thrown away on the next
//! tick. Two words for two things.

use std::collections::HashMap;

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

/// The workspace for an open [`Memory`], as of `now`.
///
/// `stats` is read **before** the graph guard is taken. [`Memory::stats`] locks
/// the graph itself, and `parking_lot`'s read lock is not recursion-safe: a
/// writer queued between the two acquires deadlocks a thread that already holds
/// one reader. Taking the figures first means only one lock is ever held here.
pub fn of_memory<Tz: TimeZone>(memory: &Memory, now: DateTime<Tz>) -> Workspace {
    let stats = memory.stats();
    let graph = memory.graph().read();
    of_graph(&graph, &stats, now)
}

/// The workspace for a graph and a clock, with nothing else behind it.
///
/// `now` carries its own time zone, and every stored instant is resolved into
/// that zone before it is placed on a calendar day. The zone is a parameter
/// rather than [`chrono::Local`] read inside so the day-boundary cases can be
/// tested at a pinned offset — the suite must not answer differently on a
/// developer's laptop than in CI, which runs in UTC.
pub fn of_graph<Tz: TimeZone>(graph: &Graph, stats: &MemoryStats, now: DateTime<Tz>) -> Workspace {
    let zone = now.timezone();
    let dates = week_dates(&now);
    let placed = placements(graph, &zone, &dates);
    let days = days(graph, &zone, &dates, &placed);
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
        threads: threads(graph, &placed),
        trickle: trickle(graph, &placed),
        // Not fed from the graph. The conversation is the chat loop's, and until
        // it can be driven from a redraw loop the panel stays as M11 left it.
        conversation: Conversation::default(),
        health: health(stats),
    }
}

/// The status bar: one word for the session's state, and how much is behind it.
///
/// Two words at most, per `1i`: "One mark, one word: reachable, saved, keeping
/// up. Never a sentence."
pub(crate) fn health(stats: &MemoryStats) -> Health {
    let (state, well) = if stats.degraded {
        (text::get("tui.health_degraded"), false)
    } else if stats.log_depth > 0 {
        (text::get("tui.health_catching_up"), false)
    } else {
        (text::get("tui.health_keeping_up"), true)
    };
    let scope = text::get("tui.scope_live").replace("{count}", &stats.node_count.to_string());
    Health {
        state: state.to_owned(),
        scope: scope.clone(),
        short_scope: scope,
        well,
    }
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
/// Once, because three separate readers want the answer — the day logs, a
/// thread's marks and the trickle's freshness — and a graph with an interaction
/// per turn of a long session is not a thing to walk three times a tick.
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
fn week_dates<Tz: TimeZone>(now: &DateTime<Tz>) -> [NaiveDate; WEEK] {
    let today = now.date_naive();
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
) -> Vec<Day> {
    let mut logs: [Vec<&Interaction>; WEEK] = std::array::from_fn(|_| Vec::new());
    let mut counts = [0usize; WEEK];
    for interaction in graph.interactions() {
        let Some(index) = placed.get(&interaction.id).and_then(|place| place.day) else {
            continue;
        };
        // The bar counts every turn, whether or not it left something to quote:
        // it measures how full the day was, not how much of it is printable.
        counts[index] = counts[index].saturating_add(1);
        if said(interaction).is_some() {
            logs[index].push(interaction);
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
            log.sort_by(|left, right| {
                left.about_time()
                    .cmp(&right.about_time())
                    .then_with(|| left.id.0.cmp(&right.id.0))
            });
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
                    .map(|interaction| {
                        Entry::at(
                            &clock(&interaction.about_time().with_timezone(zone)),
                            said(interaction).unwrap_or_default(),
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
/// An interaction with no prompt — Lambo's `record_action` opens one for work
/// that was done rather than said — has no line to contribute, and an [`Entry`]
/// with empty text is a blank row with a timestamp beside it.
fn said(interaction: &Interaction) -> Option<&str> {
    interaction
        .prompt_text
        .as_deref()
        .map(str::trim)
        .filter(|said| !said.is_empty())
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

/// What a concept is worth as a thread: how many separate turns reached it, and
/// which of the week's days those were.
struct Recurrence<'a> {
    concept: &'a Concept,
    returns: usize,
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
fn threads(graph: &Graph, placed: &HashMap<NodeId, Placed>) -> Vec<Thread> {
    let mut found: Vec<Recurrence<'_>> = graph
        .concepts()
        .filter_map(|concept| recurrence(graph, placed, concept))
        .collect();
    found.sort_by(|left, right| {
        right
            .returns
            .cmp(&left.returns)
            .then_with(|| right.day_count().cmp(&left.day_count()))
            .then_with(|| right.latest.cmp(&left.latest))
            // Total, so two equally-returned-to thoughts do not swap places
            // between one tick and the next.
            .then_with(|| left.concept.content.cmp(&right.concept.content))
            .then_with(|| left.concept.id.0.cmp(&right.concept.id.0))
    });
    found
        .into_iter()
        .take(THREADS)
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

impl Recurrence<'_> {
    /// How many of the week's days this thought came up on. Orders the list;
    /// never drawn as a number.
    fn day_count(&self) -> usize {
        self.days.iter().filter(|came_up| **came_up).count()
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
        returns: supports.len(),
        days,
        latest: latest?,
    })
}

/// "Just remembered": what the week on screen picked up, freshest first.
///
/// Windowed to the same seven days as everything else. The panel's title is a
/// claim about recency, so a graph nobody has written to since March shows
/// nothing here rather than offering March as news.
fn trickle(graph: &Graph, placed: &HashMap<NodeId, Placed>) -> Vec<Trickle> {
    let mut fresh: Vec<(DateTime<Utc>, &Concept)> = graph
        .concepts()
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
