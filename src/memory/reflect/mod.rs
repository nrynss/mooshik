//! `mooshik reflect`: a one-shot consolidation pass over the session.
//!
//! M11 built the pane, M12a filled the days, the week and the threads with
//! facts, M12b rebuilt the view on a 250 ms tick. What the screens draw is
//! still the data the graph holds — facts, edges, recurrences — and four
//! fields are deliberately empty:
//!
//! * [`crate::tui::model::Day::mood`] — how a day felt
//! * [`crate::tui::model::Day::highlights`] — the four-words-a-line gutter
//! * [`crate::tui::model::Day::notes`] — the trailing notes on the detail pane
//! * [`crate::tui::model::Thread::because`] — why a thread sits where it does
//!
//! Nothing in a graph writes an English sentence, so reflect does. It also
//! persists the paraphrase-twins consolidation the view's display fold
//! already does in-memory: the strongest survives, the rest are merged into
//! it with their original content preserved, and an audit row records what
//! happened.
//!
//! ## Storage
//!
//! Prose writes one [`prose::ProseConcept`] per field-target pair. They are
//! regular `Observation` concepts whose canonical key is
//! `mooshik-prose:<field>:<target>`. Prose is **first-write-only**: a day or
//! thread keeps the prose its first reflect run wrote, and a re-run skips it
//! (`plan_reflect` asks [`ProseIndex::get`] first). That keeps "one prose
//! concept per day" / "one per thread" true, and the canonical key is what
//! makes the stored concept readable by the view on a later pass.
//!
//! Paraphrase consolidation does **not** delete nodes: every loser is
//! rewritten with a `[merged into <winner>]: <original>` marker, its
//! incoming `Derives` edges reroute to the winner, and a `Resource` action
//! row records the merge. A second reflect pass over a graph whose twins
//! are already merged reports zero merges and writes nothing — the marker
//! tells the cluster walk to skip.
//!
//! ## Seams
//!
//! * [`Reflector`] — the prose generator. [`FixtureReflector`] is the
//!   default and the only thing the offline suite ever exercises; a
//!   `CompanionReflector` would call the OpenAI-compat client M3 built, but
//!   is live-only.
//! * [`ConsolidationPlan`] — what paraphrase consolidation would change.
//!   The dry-run reports it; the tests assert against it; the live run
//!   applies it.
//!
//! ## The write path
//!
//! reflect uses [`lambo::Memory`] the same way `mooshik stats` and
//! `mooshik chat` do — open, do the thing, close. It does **not** take the
//! view's graph guard (the guard is for the 250 ms tick; reflect is
//! one-shot) and does **not** go through the M5 tool-permission gate (the
//! gate is for tool calls; reflect is a product command).

use std::collections::HashSet;

use chrono::{DateTime, NaiveDate, Utc};
use lambo::{AgentId, Concept, ConceptType, Interaction, Memory, NodeId};

pub mod paraphrase;
pub mod prose;
pub mod snapshot;

pub use paraphrase::{
    apply_cluster, plan_paraphrase_consolidation, record_cluster_action, Cluster,
};
pub use prose::{Field, FixtureReflector, ProseConcept, ProseIndex, Target};
pub use snapshot::GraphSnapshot;

/// Generates the prose the surfaces need.
///
/// One method per surface field. The Reflector sees a [`GraphSnapshot`]
/// (lock-free, copied out from under the read guard once) and answers each
/// question deterministically. The trait is the seam between "the local
/// operator's view" and "a model calling out to a vertex"; the offline suite
/// only ever sees [`FixtureReflector`].
pub trait Reflector {
    /// How a day felt, in the day's own words. One short sentence.
    fn day_mood(&self, snapshot: &GraphSnapshot, day: NaiveDate) -> String;

    /// The four-words-a-line gutter summary. None to four short lines.
    fn day_gutter(&self, snapshot: &GraphSnapshot, day: NaiveDate) -> Vec<String>;

    /// Trailing notes on the detail pane. Paragraphs separated by blank
    /// lines; the renderer does not wrap them, so plain text.
    fn day_notes(&self, snapshot: &GraphSnapshot, day: NaiveDate) -> String;

    /// Why a thread sits where it does — one or two sentences in the user's
    /// own history.
    fn thread_reason(&self, snapshot: &GraphSnapshot, anchor: NodeId) -> String;
}

/// The result of one reflect run — what was written (or, with `dry_run`,
/// what would have been).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReflectOutcome {
    /// Prose concepts written: `(field, target)` pairs, with the text.
    pub prose_writes: Vec<ProseConcept>,
    /// Paraphrase clusters merged: survivors and their losers.
    pub paraphrase_merges: Vec<Cluster>,
}

impl ReflectOutcome {
    /// Number of prose writes.
    pub fn prose_count(&self) -> usize {
        self.prose_writes.len()
    }

    /// Number of paraphrase merges (clusters, not losers).
    pub fn merge_count(&self) -> usize {
        self.paraphrase_merges.len()
    }

    /// Total number of nodes that lost a paraphrase round (the
    /// `losers` flattened across clusters).
    pub fn merged_node_count(&self) -> usize {
        self.paraphrase_merges.iter().map(|c| c.losers.len()).sum()
    }
}

/// Plan the consolidation pass without applying it.
///
/// `dry_run = true` returns this without touching the graph; the live run
/// applies what this plan returns.
pub fn plan_reflect(
    snapshot: &GraphSnapshot,
    reflector: &dyn Reflector,
    existing_prose: &ProseIndex,
) -> ReflectOutcome {
    let days = collect_days(snapshot);
    let threads = collect_thread_anchors(snapshot);

    let mut prose_writes = Vec::new();
    for day in &days {
        for field in [Field::Mood, Field::Gutter, Field::Notes] {
            let target = Target::Day(*day);
            if existing_prose.get(field, target).is_some() {
                continue;
            }
            let text = match field {
                Field::Mood => reflector.day_mood(snapshot, *day),
                Field::Gutter => reflector.day_gutter(snapshot, *day).join("\n"),
                Field::Notes => reflector.day_notes(snapshot, *day),
                Field::ThreadReason => unreachable!(),
            };
            prose_writes.push(ProseConcept {
                field,
                target,
                text,
            });
        }
    }
    for anchor in &threads {
        let target = Target::Thread(*anchor);
        if existing_prose.get(Field::ThreadReason, target).is_some() {
            continue;
        }
        let text = reflector.thread_reason(snapshot, *anchor);
        prose_writes.push(ProseConcept {
            field: Field::ThreadReason,
            target,
            text,
        });
    }

    let plan = plan_paraphrase_consolidation(snapshot);
    ReflectOutcome {
        prose_writes,
        paraphrase_merges: plan.clusters,
    }
}

/// Run one consolidation pass: take a snapshot, plan prose writes and
/// paraphrase merges, write everything (or just plan, when `dry_run`).
///
/// `dry_run = true` answers with the planned outcome without touching the
/// graph — the operator-facing report and the tests use it.
pub async fn run_reflect(
    memory: &Memory,
    reflector: &dyn Reflector,
    dry_run: bool,
    now: DateTime<Utc>,
) -> Result<ReflectOutcome, ReflectError> {
    // One snapshot under one read guard — the Reflector answers all four
    // questions off it.
    let snapshot = {
        let graph = memory.graph().read();
        GraphSnapshot::from_graph(&graph)
    };

    // Read the prose that's already in the graph so we know which targets
    // we still need to write. A day that already has a mood is not
    // overwritten — only first-time prose is generated. The spec is
    // explicit: "one prose concept per day" / "one per thread".
    let existing_prose = ProseIndex::from_snapshot(&snapshot);
    let outcome = plan_reflect(&snapshot, reflector, &existing_prose);

    if dry_run {
        return Ok(outcome);
    }

    let agent = memory.agent().clone();

    // Apply prose writes. Each prose concept is written on its own
    // interaction so a future reader can see "the mood for day X was
    // generated at time Y" from the audit trail, and with its structured
    // `mooshik-prose:<field>:<target>` canonical key so the view can read it
    // back. One interaction per field×target tuple keeps the ordering
    // deterministic and the audit trail per-field.
    for prose in &outcome.prose_writes {
        write_prose_concept(memory, &agent, prose, now)?;
    }

    // Apply the paraphrase plan. Each cluster's merge is one critical
    // section under the graph write guard; the audit action follows.
    if !outcome.paraphrase_merges.is_empty() {
        let mut graph = memory.graph().write();
        let mut working_snapshot = snapshot.clone();
        for cluster in &outcome.paraphrase_merges {
            apply_cluster(&mut graph, cluster, &mut working_snapshot);
        }
        drop(graph);
        for cluster in &outcome.paraphrase_merges {
            let survivor_content = working_snapshot
                .content_of(cluster.survivor)
                .unwrap_or("a thought")
                .to_owned();
            let _ = record_cluster_action(memory, &agent, cluster, &survivor_content);
        }
    }

    Ok(outcome)
}

/// One of the failures a reflect run can produce.
#[derive(Debug)]
pub enum ReflectError {
    /// Lambo's own write pipeline refused (a closed session, a fenced
    /// session, store error). The detail is operator-safe.
    Backend(lambo::LamboError),
}

impl std::fmt::Display for ReflectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(error) => f.write_str(&error.to_string()),
        }
    }
}

impl std::error::Error for ReflectError {}

impl From<lambo::LamboError> for ReflectError {
    fn from(error: lambo::LamboError) -> Self {
        Self::Backend(error)
    }
}

/// Days the prose Reflector should write about — every day that has at
/// least one interaction in the snapshot.
fn collect_days(snapshot: &GraphSnapshot) -> Vec<NaiveDate> {
    let mut days: HashSet<NaiveDate> = HashSet::new();
    for interaction in &snapshot.interactions {
        let instant = interaction.event_time.unwrap_or(interaction.created_at);
        days.insert(instant.date_naive());
    }
    let mut out: Vec<NaiveDate> = days.into_iter().collect();
    out.sort();
    out
}

/// Thread anchors — the concepts that recur enough to count as threads.
///
/// We mirror the view's recurrence rule: a concept must have at least
/// [`RETURNS`] distinct interactions that wrote `Derives` edges into it,
/// and at least one of those interactions must be in the snapshot's
/// interaction set (so an old, never-returned-to thought doesn't surface
/// as a thread).
fn collect_thread_anchors(snapshot: &GraphSnapshot) -> Vec<NodeId> {
    const RETURNS: usize = 2;
    let mut out: Vec<NodeId> = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();
    for concept in &snapshot.concepts {
        if is_bookkeeping_concept(concept) {
            continue;
        }
        if ProseConcept::is_prose(concept) {
            continue;
        }
        let supports = snapshot
            .derives
            .get(&concept.id)
            .cloned()
            .unwrap_or_default();
        if supports.len() < RETURNS {
            continue;
        }
        let any_in_snapshot = supports
            .iter()
            .any(|id| snapshot.interaction_index.contains_key(id));
        if !any_in_snapshot {
            continue;
        }
        if seen.insert(concept.id) {
            out.push(concept.id);
        }
    }
    out
}

/// Local mirror of [`crate::memory::view::bookkeeping`] for the prose walk.
fn is_bookkeeping_concept(concept: &Concept) -> bool {
    concept.content.starts_with("document:")
        || concept.concept_type == ConceptType::Resource
        || concept.concept_type == ConceptType::Observation && ProseConcept::is_prose(concept)
}

/// Write a prose concept into the open session on its own interaction, with
/// the structured `mooshik-prose:<field>:<target>` canonical key the read
/// side ([`ProseIndex::from_concepts`]) parses back. Written directly rather
/// than through `derive`, which would derive the key from the prose text and
/// make the concept invisible to the view. One interaction per field×target
/// tuple keeps the audit trail per-field; the canonical key is what prevents a
/// second pass from collecting a duplicate.
fn write_prose_concept(
    memory: &Memory,
    agent: &AgentId,
    prose: &ProseConcept,
    now: DateTime<Utc>,
) -> Result<(), ReflectError> {
    let mut graph = memory.graph().write();
    let origin = NodeId::new();
    let previous_id = graph.temporal_chain().last().copied();
    graph.insert_interaction(Interaction {
        id: origin,
        session_id: memory.session().clone(),
        agent_id: agent.clone(),
        prompt_text: None,
        previous_id,
        created_at: now,
        event_time: Some(now),
    })?;
    let concept = prose.as_concept(origin, memory.session().clone(), agent.clone());
    graph.insert_concept(concept, origin)?;
    Ok(())
}

/// Read every prose concept currently in the graph and return them as an
/// index — what the view uses on every tick to surface the prose.
///
/// This is the read side of the prose schema; [`ProseConcept`] is the
/// write side.
pub fn read_prose_for_view(snapshot: &GraphSnapshot) -> ProseIndex {
    ProseIndex::from_snapshot(snapshot)
}

/// A lookup helper for the view: prose for one day, indexed by field.
pub fn prose_for_day(index: &ProseIndex, day: NaiveDate) -> DayProse {
    let target = Target::Day(day);
    DayProse {
        mood: index.get(Field::Mood, target).map(str::to_owned),
        gutter: index
            .get(Field::Gutter, target)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default(),
        notes: index
            .get(Field::Notes, target)
            .map(str::to_owned)
            .unwrap_or_default(),
    }
}

/// A lookup helper for the view: the reason for one thread, if any was
/// written.
pub fn reason_for_thread(index: &ProseIndex, anchor: NodeId) -> Option<String> {
    index
        .get(Field::ThreadReason, Target::Thread(anchor))
        .map(str::to_owned)
}

/// The prose the view surfaces for one day.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DayProse {
    pub mood: Option<String>,
    pub gutter: Vec<String>,
    pub notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_counts_match_the_plan() {
        let outcome = ReflectOutcome {
            prose_writes: vec![],
            paraphrase_merges: vec![Cluster {
                survivor: NodeId::new(),
                losers: vec![NodeId::new(), NodeId::new()],
            }],
        };
        assert_eq!(outcome.merge_count(), 1);
        assert_eq!(outcome.merged_node_count(), 2);
        assert_eq!(outcome.prose_count(), 0);
    }
}

#[cfg(test)]
#[path = "reflect_tests.rs"]
mod reflect_tests;
