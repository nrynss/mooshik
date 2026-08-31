//! Read-only graph snapshot for the Reflector seam.
//!
//! The Reflector trait must answer questions about a graph without holding
//! any lock — the view holds the lease while it asks, and a Reflector that
//! tried to take it again would deadlock. [`GraphSnapshot`] is the
//! lock-free copy: every interaction, every concept and every edge Lambo
//! knows about, plus the helpers the prose generation actually needs.
//!
//! Built from a [`lambo::Graph`] under one short read guard, then dropped
//! at the seam. The view already builds a similar copy in
//! [`super::super::view::ViewData`]; this one carries just the slices the
//! Reflector asks about, plus a `concept_by_id` index for the lookups
//! `thread_reason` needs.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use lambo::{Concept, Edge, EdgeType, Interaction, NodeId};

use crate::memory::view::Placed;

/// A pure-data snapshot of a graph for read-only reflection work.
///
/// The Reflector reads this; the consolidation pass mutates the graph and
/// updates the snapshot in lockstep to keep the cluster walk honest.
#[derive(Debug, Default, Clone)]
pub struct GraphSnapshot {
    pub interactions: Vec<Interaction>,
    pub concepts: Vec<Concept>,
    pub edges: Vec<Edge>,
    /// Concept id → index in `concepts`, for the lookups `thread_reason
    /// wants without a graph scan.
    pub concept_index: HashMap<NodeId, usize>,
    /// Interaction id → index in `interactions`.
    pub interaction_index: HashMap<NodeId, usize>,
    /// Concept id → interaction ids that wrote a `Derives` edge into it. The
    /// Reflector asks "how often did the user come back to this thought?"
    /// and the answer is the count of distinct interactions here.
    pub derives: HashMap<NodeId, Vec<NodeId>>,
    /// Interaction id → its [`Placed`] answer (just the at; the reflect
    /// module doesn't ask the week-index leg).
    pub(crate) placed: HashMap<NodeId, Placed>,
}

impl GraphSnapshot {
    /// Build the snapshot from a live graph. One linear pass per slice; the
    /// in-neighbour map is the same one [`super::super::view::edge_verdicts`]
    /// builds, just owned.
    pub fn from_graph(graph: &lambo::Graph) -> Self {
        let interactions: Vec<Interaction> = graph.interactions().cloned().collect();
        let concepts: Vec<Concept> = graph.concepts().cloned().collect();
        let edges: Vec<Edge> = graph.edges().cloned().collect();
        let concept_index = concepts
            .iter()
            .enumerate()
            .map(|(index, concept)| (concept.id, index))
            .collect();
        let interaction_index = interactions
            .iter()
            .enumerate()
            .map(|(index, interaction)| (interaction.id, index))
            .collect();
        let mut derives: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for edge in &edges {
            if edge.edge_type == EdgeType::Derives {
                derives.entry(edge.target).or_default().push(edge.source);
            }
        }
        let mut placed: HashMap<NodeId, Placed> = HashMap::with_capacity(interactions.len());
        for interaction in &interactions {
            let at = about_time(interaction);
            placed.insert(interaction.id, Placed::at_only(at));
        }
        Self {
            interactions,
            concepts,
            edges,
            concept_index,
            interaction_index,
            derives,
            placed,
        }
    }

    /// The interaction with this id, if the snapshot holds it.
    pub fn interaction(&self, id: NodeId) -> Option<&Interaction> {
        self.interaction_index
            .get(&id)
            .map(|index| &self.interactions[*index])
    }

    /// The concept with this id, if the snapshot holds it.
    pub fn concept(&self, id: NodeId) -> Option<&Concept> {
        self.concept_index
            .get(&id)
            .map(|index| &self.concepts[*index])
    }

    /// The textual content of a concept, for the prose reasons that quote it.
    pub fn content_of(&self, id: NodeId) -> Option<&str> {
        self.concept(id).map(|concept| concept.content.as_str())
    }

    /// Every interaction that wrote a `Derives` edge into `concept`. The
    /// `thread_reason` Reflector answers "how often did the user come back?".
    pub fn derived_interactions(&self, concept: NodeId) -> Vec<&Interaction> {
        self.derives
            .get(&concept)
            .map(|sources| {
                sources
                    .iter()
                    .filter_map(|id| self.interaction(*id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The day an interaction is about, in UTC. The about-time, not the flush
    /// stamp, for the reason the whole module resolves through about_time:
    /// a bootstrap flushed this afternoon is about a decade ago.
    pub fn day_of_interaction(&self, id: NodeId) -> Option<NaiveDate> {
        self.placed.get(&id).map(|placed| placed.date_naive())
    }

    /// Every interaction whose about-time lands on `day`.
    pub fn turns_on(&self, day: NaiveDate) -> usize {
        self.interactions
            .iter()
            .filter(|interaction| about_time(interaction).date_naive() == day)
            .count()
    }

    /// Every concept whose origin interaction is about `day`. Reuses
    /// [`super::super::view::about`] so the placement rule is in one place.
    pub fn concepts_on(&self, day: NaiveDate) -> Vec<&Concept> {
        let mut out = Vec::new();
        for concept in &self.concepts {
            let Some(placed) = self.placed.get(&concept.origin_interaction) else {
                continue;
            };
            if placed.date_naive() == day {
                out.push(concept);
            }
        }
        out
    }

    /// Just the content strings, filtered to one concept type. The gutter
    /// Reflector picks the four strongest by length.
    pub fn concept_contents_on(&self, day: NaiveDate, kind: lambo::ConceptType) -> Vec<String> {
        self.concepts_on(day)
            .into_iter()
            .filter(|concept| concept.concept_type == kind)
            .map(|concept| concept.content.clone())
            .collect()
    }
}

/// The instant a turn is *about*, in UTC.
///
/// Mirrors [`crate::memory::view::about`] but at the `Interaction` level —
/// `about` takes a concept and looks up its origin, this takes the turn
/// directly. Both follow the same fallback: a turn with no event time is
/// about the moment it was flushed.
fn about_time(interaction: &Interaction) -> DateTime<Utc> {
    interaction.event_time.unwrap_or(interaction.created_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_graph() -> lambo::Graph {
        let mut graph = lambo::Graph::new(lambo::SessionId::new("mooshik"));
        graph
            .insert_interaction(lambo::Interaction {
                id: NodeId::new(),
                session_id: lambo::SessionId::new("mooshik"),
                agent_id: lambo::AgentId::new("mooshik"),
                prompt_text: None,
                previous_id: None,
                created_at: Utc::now(),
                event_time: None,
            })
            .unwrap();
        graph
    }

    #[test]
    fn snapshot_captures_interactions_and_concepts() {
        let graph = test_graph();
        let snap = GraphSnapshot::from_graph(&graph);
        assert_eq!(snap.interactions.len(), 1);
        assert!(snap.concepts.is_empty());
        assert!(snap.edges.is_empty());
    }
}
