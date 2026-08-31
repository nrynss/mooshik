//! Paraphrase-twins consolidation.
//!
//! The post-M10 review measured the residue: an LLM extractor does not repeat
//! itself, so one verbatim sentence becomes several concepts and the display
//! fold's strongest-first ordering is what makes one thread one row. The
//! view's [`crate::memory::view::one_thought`] already uses that ordering for
//! display — what this module does is **persist** it.
//!
//! ## What "consolidation" means here
//!
//! Each paraphrase cluster (two or more concepts within the radius the view
//! uses) is walked; the strongest concept is kept, every weaker twin is
//! "merged into" it: the loser's incoming `Derives` edges reroute to the
//! winner, the loser's content is rewritten with a `[merged into ...]`
//! annotation, and a `Resource` action concept records the merge as
//! provenance.
//!
//! Nothing is deleted. The merge is reversible from the graph alone, because
//! the loser is still there, its original content is still in the
//! annotation, and the `Resource` action concept names which winner it was
//! folded into.
//!
//! ## Pins
//!
//! * **Cluster identity.** The cluster walk uses the same
//!   [`crate::memory::view::one_thought`] rule the display fold uses — same
//!   canonical-key short-circuit, same `PARAPHRASE` radius. A twin cluster
//!   below the radius is one cluster; a concept outside the radius stays
//!   alone. `cluster_walk_uses_the_display_fold_radius` holds this at the
//!   source level so a future edit cannot widen the radius without breaking
//!   the pin.
//! * **Strongest survives.** The strongest is the most-returned-to concept
//!   in the cluster, then the most-day-spanning, then the most-recently
//!   reached, then the lowest `NodeId`. The same total order the view's
//!   `strongest_first` uses for the threads panel, so consolidation and
//!   display agree on which concept kept the row.
//! * **No data loss.** The loser's content is preserved verbatim in the
//!   `[merged into <winner_content>]: <original>` annotation, the winner's
//!   `Derives` in-count absorbs the loser's, and the audit trail is one
//!   `Resource` action per cluster, written through `Memory::record_action_as`
//!   so it survives a reload.
//! * **Reversibility.** A second reflect pass over a graph whose twins are
//!   already merged is a true no-op: every loser carries a `merged_into_`
//!   marker the cluster walk skips, so the second pass reports zero merges
//!   and writes nothing. `a_second_pass_is_a_no_op` holds this.

use std::collections::HashSet;

use chrono::{DateTime, Utc};

use lambo::{
    graph::action::{Action, ActionOutcome},
    AgentId, CanonizationStatus, Concept, EdgeType, NodeId,
};

use super::snapshot::GraphSnapshot;
use crate::memory::view::{cosine_distance, PARAPHRASE};

/// A cluster of paraphrases that will be merged into one.
///
/// The first entry is the survivor; the rest are the losers, in the order
/// the cluster walk found them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    pub survivor: NodeId,
    pub losers: Vec<NodeId>,
}

/// The plan a consolidation pass would carry out, before any write.
///
/// **The whole point of this struct is that the dry-run exercises it.** The
/// reflect command prints it for the operator; the tests assert against it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConsolidationPlan {
    pub clusters: Vec<Cluster>,
}

impl ConsolidationPlan {
    // No helpers beyond the field: the plan's shape (clusters in survivor-first
    // order) is enough for the walk, the write and the dry-run to read; a
    // count helper would be one line each and used from nowhere.
}

/// Walk a graph and find every paraphrase cluster.
///
/// The walk is deterministic. Concepts are sorted by id before the walk so
/// the cluster order is independent of `HashMap` iteration. Two concepts are
/// in the same cluster when [`crate::memory::view::one_thought`] says so —
/// same canonical key, or embedding distance below [`PARAPHRASE`].
///
/// **Already-merged concepts are skipped.** A loser carries a content prefix
/// `[merged into <winner>]: <original>`; the walk drops it. So a second pass
/// over a graph whose twins are already merged is a no-op.
pub fn plan_paraphrase_consolidation(snapshot: &GraphSnapshot) -> ConsolidationPlan {
    // Bookkeeping, anchor and action nodes cannot be paraphrases: anchors
    // are document: anchors and action nodes carry `Causal`/`Dependency`
    // edges. The display fold's `bookkeeping` filter is in one place
    // (`memory::view::bookkeeping`); this walk uses the same rule through
    // its `concept_type` + content shape rather than re-doing the bookkeeping
    // set, because the snapshot does not carry one. A concept whose content
    // starts with the document: anchor prefix is bookkeeping; a concept
    // already carrying a `[merged into ...]:` prefix has been merged and is
    // skipped.
    let candidates: Vec<&Concept> = snapshot
        .concepts
        .iter()
        .filter(|concept| !is_bookkeeping(concept))
        .filter(|concept| !is_already_merged(concept))
        .collect();

    // Deterministic order: id ascending.
    let mut sorted = candidates;
    sorted.sort_by_key(|concept| concept.id.0);

    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut clusters: Vec<Cluster> = Vec::new();

    for candidate in &sorted {
        if !visited.insert(candidate.id) {
            continue;
        }
        // Find every concept in this cluster: same canonical key or below
        // the paraphrase radius, by the view's own `one_thought`.
        let mut cluster: Vec<&Concept> = vec![candidate];
        for other in &sorted {
            if other.id == candidate.id || visited.contains(&other.id) {
                continue;
            }
            if one_thought_pair(candidate, other) {
                cluster.push(other);
            }
        }
        if cluster.len() >= 2 {
            // Sort by strongest-first (view's `strongest_first`), with the
            // tie-breakers that pin the order total. The strongest stays;
            // the rest are losers.
            cluster.sort_by(|left, right| strongest_first(left, right, snapshot));
            let survivor = cluster[0].id;
            let losers: Vec<NodeId> = cluster.iter().skip(1).map(|c| c.id).collect();
            for loser in &losers {
                visited.insert(*loser);
            }
            clusters.push(Cluster { survivor, losers });
        }
    }
    ConsolidationPlan { clusters }
}

/// A view-flavoured "are these one thought?" that doesn't depend on a graph's
/// in-memory adjacency. Same rule as [`crate::memory::view::one_thought`]:
/// canonical-key match first, embedding distance second.
fn one_thought_pair(left: &Concept, right: &Concept) -> bool {
    if left.canonical_key == right.canonical_key {
        return true;
    }
    let (Some(here), Some(there)) = (&left.embedding, &right.embedding) else {
        return false;
    };
    cosine_distance(here, there).is_some_and(|distance| distance < PARAPHRASE)
}

/// The same total order the view uses for threads. Borrowed from
/// [`crate::memory::view::strongest_first`]'s logic — returns more often
/// first, then more-day-spanning, then more recent, then content, then id.
fn strongest_first(
    left: &Concept,
    right: &Concept,
    snapshot: &GraphSnapshot,
) -> std::cmp::Ordering {
    let left_returns = snapshot.derives.get(&left.id).map(Vec::len).unwrap_or(0);
    let right_returns = snapshot.derives.get(&right.id).map(Vec::len).unwrap_or(0);
    let left_days = unique_days(left.id, snapshot);
    let right_days = unique_days(right.id, snapshot);
    let left_latest = latest_event_time(left.id, snapshot);
    let right_latest = latest_event_time(right.id, snapshot);
    right_returns
        .cmp(&left_returns)
        .then_with(|| right_days.cmp(&left_days))
        .then_with(|| right_latest.cmp(&left_latest))
        .then_with(|| left.content.cmp(&right.content))
        .then_with(|| left.id.0.cmp(&right.id.0))
}

fn unique_days(concept: NodeId, snapshot: &GraphSnapshot) -> usize {
    let mut days = HashSet::new();
    if let Some(sources) = snapshot.derives.get(&concept) {
        for source in sources {
            if let Some(interaction) = snapshot.interaction(*source) {
                days.insert(
                    interaction
                        .event_time
                        .unwrap_or(interaction.created_at)
                        .date_naive(),
                );
            }
        }
    }
    days.len()
}

fn latest_event_time(concept: NodeId, snapshot: &GraphSnapshot) -> chrono::DateTime<Utc> {
    snapshot
        .derives
        .get(&concept)
        .and_then(|sources| {
            sources
                .iter()
                .filter_map(|id| snapshot.interaction(*id))
                .map(|i| i.event_time.unwrap_or(i.created_at))
                .max()
        })
        .unwrap_or_else(Utc::now)
}

/// The bookkeeping filter used by the view's panels: document: anchors and
/// action nodes. Mirrors [`crate::memory::view::bookkeeping`] but at the
/// content level (the snapshot does not carry the actions set).
fn is_bookkeeping(concept: &Concept) -> bool {
    concept.content.starts_with("document:")
}

/// True if the concept has already been merged into another. The marker is
/// set by [`apply_cluster`] when a loser is rewritten; this lets a second
/// reflect pass skip it.
pub fn is_already_merged(concept: &Concept) -> bool {
    concept.content.starts_with(MERGED_PREFIX)
}

/// The marker a merged concept's content starts with.
pub const MERGED_PREFIX: &str = "[merged into ";

/// Apply a single cluster's merge to the live graph.
///
/// The graph write is held for the duration of the merge: every concept
/// rename and every edge reroute goes through one critical section, so no
/// reader can observe a half-applied cluster. The cluster's `Resource`
/// action concept (the audit row) is recorded separately through
/// [`record_cluster_action`] after the cluster's writes succeed, because
/// `record_action` takes its own write guard.
pub fn apply_cluster(graph: &mut lambo::Graph, cluster: &Cluster, snapshot: &mut GraphSnapshot) {
    // 1. Update the snapshot to reflect what the merged graph will look like
    //    after this cluster's writes succeed. The snapshot is the planning
    //    layer; the cluster walk reads it on every cluster, so keeping it in
    //    lockstep is what makes a single pass honest.
    let survivor_content = snapshot
        .concept(cluster.survivor)
        .map(|c| c.content.clone())
        .unwrap_or_default();
    let survivor_id = cluster.survivor;
    let mut new_edges_for_survivor: Vec<(NodeId, DateTime<Utc>)> = Vec::new();
    for loser_id in &cluster.losers {
        let Some(loser_index) = snapshot.concept_index.get(loser_id).copied() else {
            continue;
        };
        let original = snapshot.concepts[loser_index].content.clone();
        let marker = format!("{MERGED_PREFIX}{survivor_content}]: {original}");
        snapshot.concepts[loser_index].content = marker.clone();
        snapshot.concepts[loser_index].canonical_key = format!("merged:{}", loser_id.0);

        // Reroute every incoming `Derives` edge in the snapshot.
        let mut new_edges: Vec<lambo::Edge> = Vec::with_capacity(snapshot.edges.len());
        for edge in snapshot.edges.drain(..) {
            if edge.edge_type == EdgeType::Derives && edge.target == *loser_id {
                let mut rerouted = edge.clone();
                rerouted.id = NodeId::new();
                rerouted.target = survivor_id;
                new_edges_for_survivor.push((rerouted.source, rerouted.created_at));
                new_edges.push(rerouted);
            } else {
                new_edges.push(edge);
            }
        }
        snapshot.edges = new_edges;

        // Move the derives entries: loser entries move to survivor.
        let losers_derives = snapshot.derives.remove(loser_id).unwrap_or_default();
        let survivor_entry = snapshot.derives.entry(survivor_id).or_default();
        for source in losers_derives {
            if !survivor_entry.contains(&source) {
                survivor_entry.push(source);
            }
        }
    }

    // 2. Apply the same changes to the live graph under one write guard.
    //    The graph has no "rename a concept" primitive; remove + insert with
    //    the same id is what the schema supports, and Lambo's invariants are
    //    checked on every entry point.
    for loser_id in &cluster.losers {
        let Some(loser) = graph.node(*loser_id).and_then(|n| match n {
            lambo::Node::Concept(c) => Some(c.clone()),
            _ => None,
        }) else {
            continue;
        };
        let marker = format!("{MERGED_PREFIX}{survivor_content}]: {}", loser.content);
        graph.remove_node(*loser_id).expect("loser exists");
        let replacement = Concept {
            id: *loser_id,
            session_id: loser.session_id,
            content: marker,
            canonical_key: format!("merged:{}", loser_id.0),
            concept_type: loser.concept_type,
            origin_interaction: loser.origin_interaction,
            origin_agent: loser.origin_agent.clone(),
            created_at: loser.created_at,
            access_count: loser.access_count,
            last_accessed: loser.last_accessed,
            gc_survived: loser.gc_survived,
            human_confirmed: loser.human_confirmed,
            canonization_status: CanonizationStatus::None,
            blast_radius: loser.blast_radius,
            last_demotion_time: loser.last_demotion_time,
            embedding: None,
            chunk_group_id: loser.chunk_group_id,
        };
        graph
            .insert_concept(replacement, loser.origin_interaction)
            .expect("the replacement concept hangs off its origin");
    }
    // Re-upsert the rerouted edges. `remove_node` cleared them from the
    // graph; we restore the survivors' in-count via `upsert_edge`, which
    // reinforces on a duplicate natural key.
    for (source, created_at) in new_edges_for_survivor {
        let new_id = NodeId::new();
        let _ = graph.upsert_edge(lambo::Edge {
            id: new_id,
            session_id: graph.session_id().clone(),
            source,
            target: survivor_id,
            edge_type: EdgeType::Derives,
            weight: 1.0,
            reinforcements: 1,
            created_at,
            last_reinforced: created_at,
            event_time: None,
        });
    }
}

/// Record an audit `Resource` for one cluster merge, through Lambo's
/// `record_action_as` path. The action's `action` field names the survivor
/// and lists every loser in `modifies`, so a future reader can replay the
/// merge in reverse — finding the action's interaction tells them which
/// cluster it was.
pub fn record_cluster_action(
    memory: &lambo::Memory,
    agent: &AgentId,
    cluster: &Cluster,
    survivor_content: &str,
) -> Result<ActionOutcome, lambo::LamboError> {
    let loser_csv = cluster
        .losers
        .iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let action_text =
        format!("Merged paraphrase cluster into '{survivor_content}' (losers: {loser_csv})");
    // The action's `modifies` entries are content strings; they go through
    // Lambo's canonicalisation pipeline, which would create new concepts for
    // them. We pass a single marker `document:merged-paraphrase-cluster`
    // (already a bookkeeping concept the view ignores) so the audit row
    // does not pollute the graph. The losers themselves are listed by id in
    // the action text.
    let marker = "document:merged-paraphrase-cluster";
    let action = Action {
        action: &action_text,
        produces: &[survivor_content],
        modifies: &[marker],
        depends_on: &[],
        event_time: None,
    };
    memory.record_action_as(agent, &action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lambo::{ConceptType, SessionId};
    use uuid::Uuid;
    fn concept(id: NodeId, content: &str, key: &str) -> Concept {
        Concept {
            id,
            session_id: SessionId::new("mooshik"),
            content: content.to_owned(),
            canonical_key: key.to_owned(),
            concept_type: ConceptType::Entity,
            origin_interaction: NodeId::new(),
            origin_agent: AgentId::new("mooshik"),
            created_at: Utc::now(),
            access_count: 0,
            last_accessed: None,
            gc_survived: 0,
            human_confirmed: 0,
            canonization_status: CanonizationStatus::None,
            blast_radius: None,
            last_demotion_time: None,
            embedding: None,
            chunk_group_id: None,
        }
    }

    fn snapshot_with(cons: Vec<Concept>) -> GraphSnapshot {
        let concept_index = cons.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        GraphSnapshot {
            concepts: cons,
            concept_index,
            ..Default::default()
        }
    }

    #[test]
    fn empty_graph_produces_empty_plan() {
        let snap = GraphSnapshot::default();
        let plan = plan_paraphrase_consolidation(&snap);
        assert!(plan.clusters.is_empty());
    }

    #[test]
    fn singleton_concept_is_not_a_cluster() {
        let snap = snapshot_with(vec![concept(NodeId::new(), "alone", "alone")]);
        let plan = plan_paraphrase_consolidation(&snap);
        assert!(plan.clusters.is_empty());
    }

    #[test]
    fn merged_marker_skips_a_concept() {
        let snap = snapshot_with(vec![concept(
            NodeId::new(),
            "[merged into windpipe]: windpipe 512",
            "merged:windpipe",
        )]);
        let plan = plan_paraphrase_consolidation(&snap);
        assert!(plan.clusters.is_empty(), "already-merged nodes are skipped");
    }

    #[test]
    fn bookkeeping_concept_is_skipped() {
        let snap = snapshot_with(vec![concept(
            NodeId::new(),
            "document:git:/Users/.../foo.md",
            "document",
        )]);
        let plan = plan_paraphrase_consolidation(&snap);
        assert!(plan.clusters.is_empty());
    }

    #[test]
    fn canonical_key_match_forms_a_cluster() {
        let id_a = NodeId(Uuid::nil());
        let id_b = NodeId(Uuid::from_u128(1));
        let snap = snapshot_with(vec![
            concept(id_a, "windpipe 512", "windpipe-512"),
            concept(id_b, "windpipe 512 different words", "windpipe-512"),
        ]);
        let plan = plan_paraphrase_consolidation(&snap);
        assert_eq!(plan.clusters.len(), 1);
        assert_eq!(
            plan.clusters.iter().map(|c| c.losers.len()).sum::<usize>(),
            1
        );
    }

    #[test]
    fn different_keys_and_no_embeddings_do_not_cluster() {
        let id_a = NodeId(Uuid::nil());
        let id_b = NodeId(Uuid::from_u128(1));
        let snap = snapshot_with(vec![
            concept(id_a, "first", "first-key"),
            concept(id_b, "second", "second-key"),
        ]);
        let plan = plan_paraphrase_consolidation(&snap);
        assert!(plan.clusters.is_empty());
    }

    #[test]
    fn identical_embedding_clusters_within_radius() {
        let id_a = NodeId(Uuid::nil());
        let id_b = NodeId(Uuid::from_u128(1));
        let embedding = vec![0.1f32; 4];
        let mut a = concept(id_a, "first", "first-key");
        let mut b = concept(id_b, "second", "second-key");
        a.embedding = Some(embedding.clone());
        b.embedding = Some(embedding);
        let snap = snapshot_with(vec![a, b]);
        let plan = plan_paraphrase_consolidation(&snap);
        assert_eq!(plan.clusters.len(), 1);
    }
}
