//! The workspace view's own tests, in a sibling file — the same split
//! `screen/week.rs` and `screen/week_tests.rs` use, and for the same reason.
//!
//! **Every clock in here is pinned, and the zone is not UTC.** The view is a
//! function of a graph and an instant, so the suite supplies both; a test that
//! read the machine's own zone would answer one way on a developer's laptop and
//! another in CI, which is the shape of a green that means nothing. The zone is
//! +05:45 on purpose: it is not a whole number of hours, so a day boundary
//! computed by rounding the timestamp instead of converting the calendar date
//! comes out wrong by more than it could by luck.
//!
//! A fixed offset cannot tell a *zone* from an offset, though, and that is the
//! other way to get a day boundary wrong — so the calendar itself is pinned next
//! door in `view_clock_tests.rs`, against a zone with a real transition in it.

use super::*;

use chrono::FixedOffset;
use lambo::{AgentId, CanonizationStatus, ConceptType, SessionId};

/// A zone that is neither UTC nor a whole hour from it.
pub(super) fn zone() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 45 * 60).expect("+05:45 is a zone")
}

/// The instant every test draws: Thursday 27 August 2026, 14:22 local.
pub(super) fn now() -> DateTime<FixedOffset> {
    zone()
        .with_ymd_and_hms(2026, 8, 27, 14, 22, 0)
        .single()
        .expect("an unambiguous local instant")
}

/// `days` days and `hours` hours before [`now`], as the store would hold it.
pub(super) fn before(days: i64, hours: i64) -> DateTime<Utc> {
    (now() - chrono::Duration::days(days) - chrono::Duration::hours(hours)).with_timezone(&Utc)
}

/// Long before any week on screen — the bootstrap corpus's own timeline.
pub(super) fn long_ago() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2015, 6, 15, 12, 0, 0)
        .single()
        .expect("an instant in 2015")
}

/// A graph built one turn at a time, keeping the temporal chain Lambo requires.
pub(super) struct Corpus {
    pub(super) graph: Graph,
    tail: Option<NodeId>,
}

impl Corpus {
    pub(super) fn new() -> Self {
        Self {
            graph: Graph::new(SessionId::new("mooshik")),
            tail: None,
        }
    }

    /// One turn: what was said, when Lambo flushed it, and what it is about.
    ///
    /// `event_time` of `None` is the live case — the turn is about the moment it
    /// was recorded — which is the fallback rule `about_time` implements.
    pub(super) fn turn(
        &mut self,
        said: Option<&str>,
        flushed_at: DateTime<Utc>,
        event_time: Option<DateTime<Utc>>,
    ) -> NodeId {
        let id = NodeId::new();
        self.graph
            .insert_interaction(Interaction {
                id,
                session_id: SessionId::new("mooshik"),
                agent_id: AgentId::new("mooshik"),
                prompt_text: said.map(str::to_owned),
                previous_id: self.tail,
                created_at: flushed_at,
                event_time,
            })
            .expect("the turn joins the chain");
        self.tail = Some(id);
        id
    }

    /// A concept produced by `origin`, reusing `id` when the same thought is
    /// reached again — which is what puts a second `Derives` edge on it.
    pub(super) fn thought(
        &mut self,
        id: NodeId,
        content: &str,
        origin: NodeId,
        created_at: DateTime<Utc>,
    ) {
        self.typed_thought(id, content, origin, created_at, ConceptType::Entity, None);
    }

    /// The same, with the two fields the panels read for identity: a concept
    /// type (`Observation` is the one Lambo lets share a canonical key) and the
    /// key itself.
    fn typed_thought(
        &mut self,
        id: NodeId,
        content: &str,
        origin: NodeId,
        created_at: DateTime<Utc>,
        concept_type: ConceptType,
        canonical_key: Option<&str>,
    ) {
        let canonical_key = canonical_key
            .map(str::to_owned)
            .unwrap_or_else(|| content.to_lowercase());
        self.graph
            .insert_concept(
                Concept {
                    id,
                    session_id: SessionId::new("mooshik"),
                    content: content.to_owned(),
                    canonical_key,
                    concept_type,
                    origin_interaction: origin,
                    origin_agent: AgentId::new("mooshik"),
                    created_at,
                    access_count: 0,
                    last_accessed: None,
                    gc_survived: 0,
                    human_confirmed: 0,
                    canonization_status: CanonizationStatus::None,
                    blast_radius: None,
                    last_demotion_time: None,
                    embedding: None,
                    chunk_group_id: None,
                },
                origin,
            )
            .expect("the thought hangs off its turn");
    }

    /// A thought reached by `turns` separate turns, each at its own instant.
    fn returned_to(&mut self, content: &str, turns: &[DateTime<Utc>]) -> NodeId {
        let id = NodeId::new();
        for at in turns {
            let origin = self.turn(Some(content), *at, Some(*at));
            self.thought(id, content, origin, *at);
        }
        id
    }

    /// The same, for a thought Lambo has already judged to be one thing said two
    /// ways: two concepts, one canonical key. Lambo's schema allows a shared key
    /// only for `Observation`s, which is where a demoted chunk and its restatement
    /// land.
    fn returned_to_as(&mut self, content: &str, key: &str, turns: &[DateTime<Utc>]) -> NodeId {
        let id = NodeId::new();
        for at in turns {
            let origin = self.turn(Some(content), *at, Some(*at));
            self.typed_thought(
                id,
                content,
                origin,
                *at,
                ConceptType::Observation,
                Some(key),
            );
        }
        id
    }

    /// One turn that recorded an action, in `record_action`'s own shape: the
    /// action string on the interaction, a `Resource` concept carrying the same
    /// string, and a `Causal` edge from it to what the action produced.
    fn recorded_action(&mut self, action: &str, produced: &str, at: DateTime<Utc>) {
        let origin = self.turn(Some(action), at, Some(at));
        let node = NodeId::new();
        self.typed_thought(node, action, origin, at, ConceptType::Resource, None);
        let target = NodeId::new();
        self.typed_thought(target, produced, origin, at, ConceptType::Entity, None);
        self.graph
            .upsert_edge(lambo::Edge {
                id: NodeId::new(),
                session_id: SessionId::new("mooshik"),
                source: node,
                target,
                edge_type: EdgeType::Causal,
                weight: 1.0,
                reinforcements: 1,
                created_at: at,
                last_reinforced: at,
                event_time: Some(at),
            })
            .expect("the action produced the resource");
    }

    pub(super) fn view(&self) -> Workspace {
        of_graph(&figures(), &self.graph, now())
    }
}

/// A healthy session's figures, so the status bar has something true to say.
pub(super) fn figures() -> MemoryStats {
    MemoryStats {
        session: SessionId::new("mooshik"),
        agent: AgentId::new("mooshik"),
        flush_lag: std::time::Duration::from_millis(0),
        log_depth: 0,
        flush_depth: 0,
        dead_lettered: 0,
        degraded: false,
        node_count: 214,
        edge_count: 0,
        concept_count: 0,
        canonical_count: 0,
        embedded_concepts: 0,
        epoch: 0,
        daemon_cycles: 0,
        canonization_cycles: 0,
        canonization_failures: 0,
    }
}

/// Every line of every day's log, flattened, so a test can ask what the week
/// says without caring which column said it.
pub(super) fn logged(workspace: &Workspace) -> Vec<String> {
    workspace
        .week
        .days
        .iter()
        .flat_map(|day| day.entries.iter().map(|entry| entry.text.clone()))
        .collect()
}

pub(super) fn trickled(workspace: &Workspace) -> Vec<String> {
    workspace
        .trickle
        .iter()
        .map(|line| line.text.clone())
        .collect()
}

pub(super) fn threaded(workspace: &Workspace) -> Vec<String> {
    workspace
        .threads
        .iter()
        .map(|thread| thread.summary.clone())
        .collect()
}

/// Draw a workspace at the sizes the design names, so a shape that renders
/// nowhere cannot pass as a shape.
pub(super) fn draws_everywhere(workspace: &Workspace) {
    use crate::tui::{app::App, grid::Grid, screen::chrome::View};
    use ratatui::{buffer::Buffer, layout::Rect};

    for (view, width, height) in [
        (View::Today, 120u16, 40u16),
        (View::Today, 80, 24),
        (View::Week, 120, 40),
        (View::Week, 80, 24),
    ] {
        let mut app = App::new(workspace.clone());
        app.view = view;
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        let area = buf.area;
        app.draw(&mut Grid::new(&mut buf, area));
    }
}

/// The week ends on the day the reader is in, and names its own seven days.
///
/// Both halves matter. The last column is today because every other placement
/// on screen — the ribbon, a thread's marks, "just remembered" — is an answer
/// about what has already happened; and the heads are the week's own because
/// the header row they feed used to be a fixed Friday-first string that is true
/// of the design's Thursday and of no other day.
#[test]
fn the_week_ends_on_today_and_names_its_own_days() {
    let workspace = Corpus::new().view();
    assert_eq!(workspace.week.days.len(), WEEK);
    let labels: Vec<&str> = workspace
        .week
        .days
        .iter()
        .map(|day| day.short_label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["Fri 21", "Sat 22", "Sun 23", "Mon 24", "Tue 25", "Wed 26", "Thu 27"]
    );
    assert_eq!(
        workspace.week.day_heads,
        ["Fri", "Sat", "Sun", "Mon", "Tue", "Wed", "Thu"]
    );
    assert_eq!(workspace.week.label, "21-27 August");
    assert_eq!(workspace.week.selected, WEEK - 1);
    assert_eq!(workspace.today.day_of_month, "27");
    assert_eq!(workspace.today.long_label, "Thursday 27 August");
    assert_eq!(workspace.now.long_date, "Thursday 27 August");
    assert_eq!(workspace.now.short_date, "Thu 27 Aug");
    assert_eq!(workspace.now.time, "14:22");
    // The Today panel and the last ribbon column are one day, formatted once:
    // `screen::today::today_index` matches them by `day_of_month`.
    assert_eq!(
        workspace.today.day_of_month,
        workspace.week.days[WEEK - 1].day_of_month
    );
}

/// A week that crosses a month names both, because "29-4 September" is a range
/// that does not exist.
#[test]
fn a_week_that_crosses_a_month_names_both_of_them() {
    let corpus = Corpus::new();
    let across = zone()
        .with_ymd_and_hms(2026, 9, 2, 9, 0, 0)
        .single()
        .expect("an unambiguous local instant");
    let workspace = of_graph(&figures(), &corpus.graph, across);
    assert_eq!(workspace.week.label, "27 August - 2 September");
    assert_eq!(workspace.week.days[0].short_label, "Thu 27");
    assert_eq!(workspace.week.days[WEEK - 1].short_label, "Wed 2");
}

/// A turn is placed on the day its **event time** falls on, not the day it was
/// flushed — the M9 lesson, at the only place the TUI could relearn it.
///
/// The corpus is the bootstrap shape: three turns about three separate days of
/// this week, all flushed within a second of each other just now. Reading
/// `created_at` would stack all three onto today.
#[test]
fn a_turn_is_placed_by_the_day_it_is_about_not_the_day_it_was_flushed() {
    let mut corpus = Corpus::new();
    let flushed = before(0, 0);
    corpus.turn(Some("Monday's postmortem"), flushed, Some(before(3, 2)));
    corpus.turn(Some("Tuesday's rewrite"), flushed, Some(before(2, 5)));
    corpus.turn(Some("This morning's standup"), flushed, Some(before(0, 6)));

    let workspace = corpus.view();
    let days = &workspace.week.days;
    assert_eq!(days[3].entries.len(), 1, "Monday: {:?}", days[3].entries);
    assert_eq!(days[3].entries[0].text, "Monday's postmortem");
    assert_eq!(days[4].entries.len(), 1, "Tuesday: {:?}", days[4].entries);
    assert_eq!(days[6].entries.len(), 1, "today: {:?}", days[6].entries);
    assert_eq!(days[6].entries[0].text, "This morning's standup");
    // And the day they were all flushed on holds only the one that is about it.
    assert_eq!(workspace.today.entries.len(), 1);
}

/// A turn with no event time is about the moment it was recorded, which is the
/// fallback rule and the whole of a live session.
#[test]
fn a_turn_with_no_event_time_lands_on_the_day_it_was_recorded() {
    let mut corpus = Corpus::new();
    corpus.turn(Some("Said out loud today"), before(0, 1), None);
    corpus.turn(Some("Said out loud on Sunday"), before(4, 3), None);

    let workspace = corpus.view();
    assert_eq!(
        workspace.week.days[6].entries[0].text,
        "Said out loud today"
    );
    assert_eq!(
        workspace.week.days[2].entries[0].text,
        "Said out loud on Sunday"
    );
}

/// A concept is placed by the turn that produced it, because it carries no
/// clock of its own worth reading.
///
/// This is the test that would catch a mapping that lies. A `Concept` has a
/// `created_at` and no `event_time`, so the obvious reading — sort the concepts
/// by `created_at` — is available, compiles, and is wrong the moment a
/// historical corpus is ingested: every concept in the M8 graph was flushed
/// this afternoon and is about a decade ago. The 2015 thought below is freshly
/// flushed and must appear nowhere on a screen showing this week; the live one
/// beside it is the control that proves the panel is not simply empty.
#[test]
fn a_concept_is_placed_by_the_turn_that_produced_it() {
    let mut corpus = Corpus::new();
    let just_now = before(0, 0);

    let ancient = corpus.turn(Some("A commit from 2015"), just_now, Some(long_ago()));
    corpus.thought(
        NodeId::new(),
        "The ring held 512 in flight",
        ancient,
        just_now,
    );

    let live = corpus.turn(Some("Something said this morning"), just_now, None);
    corpus.thought(NodeId::new(), "Drinks are off", live, just_now);

    let workspace = corpus.view();
    assert_eq!(
        trickled(&workspace),
        ["Drinks are off"],
        "a thought about 2015 was called just remembered"
    );
    assert!(
        !logged(&workspace).iter().any(|line| line.contains("2015")),
        "a turn about 2015 landed in this week: {:?}",
        logged(&workspace)
    );
}

/// A thought reached once has arrived, not come back.
#[test]
fn a_thought_reached_once_is_not_a_thread() {
    let mut corpus = Corpus::new();
    corpus.returned_to("Reached once", &[before(1, 0)]);
    assert!(
        corpus.view().threads.is_empty(),
        "one turn was counted as coming back"
    );

    corpus.returned_to("Reached twice", &[before(2, 0), before(1, 0)]);
    assert_eq!(threaded(&corpus.view()), ["Reached twice"]);
}

/// A thought the user kept returning to months ago is not what keeps coming
/// back *this week*, whatever its total.
///
/// The panel draws seven marks under seven day columns, so a thread with none
/// of them set reads as "this never comes up" beside a claim that it is the
/// strongest thing there is. The four-times thought below is the control: it is
/// weaker by every total and still the only thread, because it is the only one
/// the week on screen can show.
#[test]
fn an_old_thought_is_not_this_weeks_thread() {
    let mut corpus = Corpus::new();
    let old = long_ago();
    corpus.returned_to(
        "The 512 cap",
        &[
            old,
            old + chrono::Duration::days(30),
            old + chrono::Duration::days(60),
            old + chrono::Duration::days(90),
            old + chrono::Duration::days(120),
        ],
    );
    corpus.returned_to("Block, never drop", &[before(3, 0), before(1, 0)]);

    assert_eq!(threaded(&corpus.view()), ["Block, never drop"]);
}

/// The list is ordered by how often the user came back, and the count never
/// leaves this module — the position is the encoding.
#[test]
fn threads_are_ordered_by_how_often_they_come_back() {
    let mut corpus = Corpus::new();
    corpus.returned_to("Twice", &[before(4, 0), before(1, 0)]);
    corpus.returned_to(
        "Four times",
        &[before(5, 0), before(3, 0), before(2, 0), before(0, 3)],
    );
    corpus.returned_to("Three times", &[before(6, 0), before(2, 1), before(0, 2)]);

    let workspace = corpus.view();
    assert_eq!(threaded(&workspace), ["Four times", "Three times", "Twice"]);
    // The marks are the days it came up on, in the same order as the columns.
    assert_eq!(
        workspace.threads[0].days,
        [false, true, false, true, true, false, true]
    );
    assert_eq!(
        workspace.threads[2].days,
        [false, false, true, false, false, true, false]
    );
    // Nothing is written about why: that sentence is M12c's.
    assert!(workspace
        .threads
        .iter()
        .all(|thread| thread.because.is_empty()));
    assert!(workspace
        .threads
        .iter()
        .all(|thread| thread.leaned_on.is_empty()));
}

/// One thought takes one row, however many ways it was said.
///
/// Two concepts under one canonical key is Lambo saying they are the same
/// thing. The panel has five slots and this one holds four other thoughts, so an
/// unfolded list would spend two of them saying one thing twice — and would rank
/// the pair *below* the four-support thought, because neither copy carries more
/// than two supports on its own.
#[test]
fn a_thought_said_two_ways_takes_one_row_and_keeps_both_days() {
    let mut corpus = Corpus::new();
    corpus.returned_to(
        "Four times",
        &[before(6, 0), before(5, 0), before(4, 0), before(3, 0)],
    );
    corpus.returned_to_as(
        "The ring caps at 512",
        "ring 512",
        &[before(2, 0), before(2, 1), before(1, 5)],
    );
    corpus.returned_to_as(
        "512 is the ring's cap",
        "ring 512",
        &[before(1, 0), before(0, 1)],
    );

    let workspace = corpus.view();
    // Folded: five supports against four, so the pair takes the top row, and it
    // takes exactly one row. The row is the stronger copy's, because that is the
    // one that earned it.
    assert_eq!(
        threaded(&workspace),
        ["The ring caps at 512", "Four times"],
        "a thought said two ways took two rows"
    );
    // The marks are both copies' days, which is the whole point of folding
    // rather than dropping: the row is about the thought, not about the wording.
    assert_eq!(
        workspace.threads[0].days,
        [false, false, false, false, true, true, true]
    );
}

/// The second leg of the same question, at the radius post-M10 measured.
///
/// An LLM extractor does not repeat itself, so the copies of one fact carry
/// different canonical keys and only the vectors can see they are one thought.
/// The radius is 0.02: 0.01 apart is a paraphrase and 0.05 apart is two thoughts,
/// and a copy with no vector yet — writes acknowledge before the embedder runs —
/// is not folded by this leg at all.
#[test]
fn two_thoughts_inside_the_paraphrase_radius_are_one_thought() {
    fn vector(tilt: f32) -> Concept {
        Concept {
            id: NodeId::new(),
            session_id: SessionId::new("mooshik"),
            content: "the ring caps at 512".to_owned(),
            canonical_key: format!("key {tilt}"),
            concept_type: ConceptType::Entity,
            origin_interaction: NodeId::new(),
            origin_agent: AgentId::new("mooshik"),
            created_at: before(0, 0),
            access_count: 0,
            last_accessed: None,
            gc_survived: 0,
            human_confirmed: 0,
            canonization_status: CanonizationStatus::None,
            blast_radius: None,
            last_demotion_time: None,
            embedding: Some(vec![1.0, tilt, 0.0]),
            chunk_group_id: None,
        }
    }

    let here = vector(0.0);
    // cos(θ) = 1/√(1+t²), so t = 0.1414 is ~0.01 away and t = 0.3244 ~0.05.
    let paraphrase = vector(0.1414);
    let other = vector(0.3244);
    assert!(
        one_thought(&here, &paraphrase),
        "a paraphrase took its own row"
    );
    assert!(
        !one_thought(&here, &other),
        "two thoughts were folded into one"
    );

    let unembedded = Concept {
        embedding: None,
        ..vector(0.1414)
    };
    assert!(
        !one_thought(&here, &unembedded),
        "a concept with no vector was folded on a distance nothing measured"
    );
    // And the identity leg answers without one, because it is Lambo's judgement
    // and not a measurement.
    assert!(one_thought(
        &here,
        &Concept {
            embedding: None,
            canonical_key: here.canonical_key.clone(),
            ..vector(9.0)
        }
    ));
}

/// Neither panel draws the engine's record of its own work.
///
/// This is the corpus the product actually produces: the bootstrap ingester
/// reads a document, hangs the facts it extracted off a `document:<path>` anchor,
/// and records the ingest as an action. The anchor gains a support from every
/// turn that touched the document, so it *wins* the recurrence ranking outright,
/// and both it and the action string carry an absolute path out of the reader's
/// home directory onto a pane they leave open beside their work.
#[test]
fn provenance_is_not_a_thought_and_reaches_neither_panel() {
    let mut corpus = Corpus::new();
    let anchor = NodeId::new();
    let path = "document:file:/Users/neom/notes/windpipe-design.md";
    // Three days of reading one document: the anchor collects three supports,
    // the fact two.
    for day in [3i64, 2, 1] {
        let turn = corpus.turn(
            Some("read the design note"),
            before(0, 0),
            Some(before(day, 1)),
        );
        corpus.thought(anchor, path, turn, before(day, 1));
    }
    corpus.returned_to(
        "Overflow writers block instead of dropping",
        &[before(2, 2), before(1, 2)],
    );
    corpus.recorded_action(
        "Ingested file document:git:/Users/neom/work/lambo#4c6fc93",
        "document:git:/Users/neom/work/lambo#4c6fc93",
        before(1, 3),
    );

    let workspace = corpus.view();
    assert_eq!(
        threaded(&workspace),
        ["Overflow writers block instead of dropping"],
        "provenance ranked as something the user keeps coming back to"
    );
    assert_eq!(
        trickled(&workspace),
        ["Overflow writers block instead of dropping"],
        "provenance was offered as something just remembered"
    );
    // And nothing anywhere on the workspace names the reader's home directory.
    let everywhere = format!(
        "{:?}",
        (
            threaded(&workspace),
            trickled(&workspace),
            logged(&workspace)
        )
    );
    assert!(!everywhere.contains("/Users/"), "{everywhere}");
}

/// A day's log quotes what was said, and a `derive` says nothing — it restates
/// the concepts it is about to write, joined with a semicolon.
///
/// The corpus is what `mooshik serve`'s own MCP surface writes: a derive of two
/// concepts, whose interaction carries `"a; b"`, and an action, whose
/// interaction carries the action string. Neither is a turn. The third turn is
/// the control — words no concept in the graph carries — and it is the shape
/// that fills this panel the moment something records a real turn.
#[test]
fn a_turn_that_restates_what_it_wrote_is_not_a_days_log() {
    let mut corpus = Corpus::new();
    let derived = corpus.turn(
        Some("The ring holds 512 in flight; Overflow writers block"),
        before(0, 0),
        Some(before(0, 3)),
    );
    corpus.thought(
        NodeId::new(),
        "The ring holds 512 in flight",
        derived,
        before(0, 3),
    );
    corpus.thought(
        NodeId::new(),
        "Overflow writers block",
        derived,
        before(0, 3),
    );
    corpus.recorded_action(
        "Ingested file document:file:/Users/neom/notes/windpipe.md",
        "document:file:/Users/neom/notes/windpipe.md",
        before(0, 2),
    );
    corpus.turn(
        Some("Postmortem's done, and it's short"),
        before(0, 0),
        Some(before(0, 1)),
    );

    assert_eq!(
        logged(&corpus.view()),
        ["Postmortem's done, and it's short"]
    );
}

/// One concept whose content carries the separator is still an echo of itself.
///
/// The whole prompt is tested before it is split, because "a; b" is a legal
/// thing for an extractor to emit as one concept — and splitting first would put
/// the join back on screen through the one case it cannot parse.
#[test]
fn a_single_concept_containing_the_separator_is_still_an_echo() {
    let mut corpus = Corpus::new();
    let turn = corpus.turn(
        Some("The ring caps at 512; overflow blocks"),
        before(0, 0),
        Some(before(0, 1)),
    );
    corpus.thought(
        NodeId::new(),
        "The ring caps at 512; overflow blocks",
        turn,
        before(0, 1),
    );
    assert!(logged(&corpus.view()).is_empty());
}

/// "Just remembered" is freshest first, on the turn's own clock.
#[test]
fn the_trickle_is_freshest_first() {
    let mut corpus = Corpus::new();
    for (content, at) in [
        ("Oldest", before(5, 0)),
        ("Newest", before(0, 1)),
        ("Middle", before(2, 0)),
    ] {
        let origin = corpus.turn(Some(content), before(0, 0), Some(at));
        corpus.thought(NodeId::new(), content, origin, before(0, 0));
    }
    assert_eq!(trickled(&corpus.view()), ["Newest", "Middle", "Oldest"]);
}

/// The ribbon is a shape read against the week's own busiest day, so a quiet
/// week is not flattened onto the baseline and a loud one does not saturate.
#[test]
fn the_ribbon_is_measured_against_the_busiest_day_of_its_own_week() {
    let mut corpus = Corpus::new();
    for _ in 0..8 {
        corpus.turn(Some("busy"), before(0, 0), Some(before(2, 0)));
    }
    for _ in 0..4 {
        corpus.turn(Some("half"), before(0, 0), Some(before(1, 0)));
    }
    // One turn against a day of eight rounds to nothing on the share alone.
    corpus.turn(Some("barely"), before(0, 0), Some(before(0, 4)));

    let days = corpus.view().week.days;
    assert_eq!(days[4].load.glyph(), '█', "the busiest day is the full bar");
    assert_eq!(days[5].load.glyph(), '▅', "half the busiest is mid-ramp");
    // An empty day still draws: a zero would be a hole in the ribbon.
    assert_eq!(days[0].load.glyph(), '▁');
    // And a day that happened never draws the empty day's bar, whatever the
    // share rounds to — the ribbon's whole question is which days happened.
    assert_eq!(
        days[6].load.glyph(),
        '▂',
        "a single turn read as an empty day"
    );
    for day in &days {
        assert!(Load::BARS.contains(&day.load.glyph()));
        // The tone is never `Hard`: nothing in a graph decides a day was hard,
        // and the ribbon brightens today from its index anyway.
        assert!(!matches!(day.load.tone, Tone::Hard));
    }
}

/// A week with no busiest day draws flat at the top of its own scale, and that
/// is the decision rather than an accident of the arithmetic.
///
/// The floor is defended at length in `bar_level`: a day that happened must
/// never draw the empty day's glyph. The ceiling is the same argument from the
/// other end — the height is a share of this week and carries no absolute
/// meaning, so seven single-turn days are each the whole of their own week. What
/// would be wrong is a special case: capping a flat week would put two rows
/// between `[9; 7]` and a week with one quieter day.
#[test]
fn a_flat_week_is_drawn_flat_at_the_top_of_its_own_scale() {
    for turns in [1, 9] {
        let mut corpus = Corpus::new();
        for day in 0..7i64 {
            for _ in 0..turns {
                corpus.turn(Some("even"), before(0, 0), Some(before(day, 1)));
            }
        }
        let glyphs: String = corpus
            .view()
            .week
            .days
            .iter()
            .map(|day| day.load.glyph())
            .collect();
        assert_eq!(glyphs, "███████", "{turns} turns a day");
    }
}

/// A turn with nothing said contributes to the shape of the day and not to its
/// log — an entry with no text is a timestamp beside a blank row.
#[test]
fn a_turn_with_nothing_said_fills_no_line() {
    let mut corpus = Corpus::new();
    corpus.turn(None, before(0, 0), Some(before(0, 1)));
    corpus.turn(Some("   "), before(0, 0), Some(before(0, 1)));
    corpus.turn(Some("Rode in"), before(0, 0), Some(before(0, 2)));

    let workspace = corpus.view();
    assert_eq!(logged(&workspace), ["Rode in"]);
    // All three still counted toward the bar, which measures how full the day
    // was rather than how much of it is quotable.
    assert!(workspace.today.load.glyph() != '▁');
}

/// An empty graph gives a workspace the screens can draw: seven labelled days,
/// a drawable bar on each, a selection inside the week, and no invented content.
#[test]
fn an_empty_graph_gives_a_well_formed_workspace() {
    let workspace = Corpus::new().view();

    assert!(workspace.threads.is_empty());
    assert!(workspace.trickle.is_empty());
    assert!(workspace.conversation.turns.is_empty());
    assert_eq!(workspace.health.scope, "214 things remembered");
    assert_eq!(workspace.health.short_scope, "214 remembered");
    assert!(workspace.health.well);
    assert!(workspace.week.selected < workspace.week.days.len());
    for day in &workspace.week.days {
        assert!(!day.short_label.is_empty());
        assert!(!day.long_label.is_empty());
        assert!(!day.day_of_month.is_empty());
        assert!(day.entries.is_empty());
        // Nothing writes prose or observes weather this milestone.
        assert!(day.weather.is_none());
        assert!(day.mood.is_none());
        assert!(day.highlights.is_empty());
        assert!(day.notes.is_empty());
    }
    draws_everywhere(&workspace);
}

/// And a filled one draws at every size too, which is the other half: a week of
/// real prose is what finds a panel that only ever met a fixture.
#[test]
fn a_filled_workspace_draws_at_every_screen_size() {
    let mut corpus = Corpus::new();
    for day in 0..7i64 {
        for hour in 0..3i64 {
            corpus.turn(
                Some("The ring overflowed in production and nothing was dropped, because writers wait"),
                before(0, 0),
                Some(before(day, hour)),
            );
        }
    }
    corpus.returned_to(
        "The ring holds 512 in flight; overflow blocks, never drops",
        &[before(5, 0), before(3, 0), before(0, 1)],
    );
    draws_everywhere(&corpus.view());
}

/// A day's log carries no more lines than any panel can draw.
///
/// There is no scroll: `aside::entries` stops at the panel's last interior row
/// and no key reaches past it, so a thousand-turn day used to build a thousand
/// `Entry` values — two `String`s each — for a pane that draws about twenty rows.
/// The kept end is the early one, because that is the end the panels draw from.
#[test]
fn a_days_log_is_bounded_by_what_a_panel_can_draw() {
    let mut corpus = Corpus::new();
    for minute in 0..(LOG + 20) {
        let at = before(0, 6) + chrono::Duration::minutes(minute as i64);
        corpus.turn(Some(&format!("turn {minute}")), before(0, 0), Some(at));
    }
    let workspace = corpus.view();
    assert_eq!(workspace.today.entries.len(), LOG);
    assert_eq!(workspace.today.entries[0].text, "turn 0");
    // The bar still counts the whole day: the cap is about what is drawn, not
    // about how full the day was.
    assert_eq!(workspace.today.load.glyph(), '█');
}
