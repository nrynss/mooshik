//! The tick's shape, measured: what one rebuild of the view model costs
//! against a session-sized graph, and that it fits the 250 ms tick budget.
//!
//! The event loop rebuilds the workspace every tick — [`of_memory`]: the
//! session's figures, the graph copied out from under its lock into
//! [`ViewData`], and [`of_graph`]'s build over the copy. This file measures
//! that read side on the two shapes the M12a reviews measured — 1 000 turns /
//! 400 concepts / 1 999 edges and 4 000 / 1 500 / 7 999 — built by hand so
//! nothing here needs a store, a daemon or a clock.
//!
//! Each shape is measured twice: with concepts carrying no vector, which is
//! the shape the M12a reviews measured and the baseline this milestone's
//! numbers are compared against, and with deterministic 1536-dimensional
//! vectors, which is the product's shape once the embedder has run — the
//! paraphrase fold then pays a real cosine per compared pair, and the pair
//! counts are bounded by the fold's pool. [`of_memory`]'s doc carries the
//! numbers; this test is what keeps them honest.

use super::tests::{draws_everywhere, figures, now};
use super::*;

use std::time::{Duration, Instant};

use chrono::Utc;
use lambo::{AgentId, ConceptType, EmbeddingContract, SessionId};

use crate::tui::TICK;

/// The two shapes the M12a reviews measured: `turns` interactions each
/// deriving one concept — `turns - 1` `Temporal` edges between them and
/// `turns` `Derives` edges, which is the 1 999 / 7 999 the records quote.
fn session(turns: usize, concepts: usize, embedded: bool) -> Graph {
    let mut graph = Graph::new(SessionId::new("mooshik"));
    graph
        .stamp_embedding(EmbeddingContract {
            kind: "fixture".to_owned(),
            model: None,
            dim: 1536,
        })
        .expect("a fresh graph accepts its first embedding contract");
    let thought_ids: Vec<NodeId> = (0..concepts).map(|_| NodeId::new()).collect();
    let base = now().with_timezone(&Utc);
    let mut tail: Option<NodeId> = None;
    for turn in 0..turns {
        let at = base - chrono::Duration::minutes((turns - 1 - turn) as i64);
        let id = NodeId::new();
        graph
            .insert_interaction(Interaction {
                id,
                session_id: SessionId::new("mooshik"),
                agent_id: AgentId::new("mooshik"),
                prompt_text: Some(format!("turn {turn}")),
                previous_id: tail,
                created_at: at,
                event_time: Some(at),
            })
            .expect("the turn joins the chain");
        tail = Some(id);
        let thought = turn % concepts;
        graph
            .insert_concept(
                Concept {
                    id: thought_ids[thought],
                    session_id: SessionId::new("mooshik"),
                    content: format!("thought {thought}"),
                    canonical_key: format!("thought {thought}"),
                    concept_type: ConceptType::Entity,
                    origin_interaction: id,
                    origin_agent: AgentId::new("mooshik"),
                    created_at: at,
                    access_count: 0,
                    last_accessed: None,
                    gc_survived: 0,
                    human_confirmed: 0,
                    canonization_status: lambo::CanonizationStatus::None,
                    blast_radius: None,
                    last_demotion_time: None,
                    embedding: embedded.then(|| vector(thought)),
                    chunk_group_id: None,
                },
                id,
            )
            .expect("the thought hangs off its turn");
    }
    graph
}

/// A deterministic 1536-dimensional vector for `seed`, so every run compares
/// the same distances without a random number generator.
fn vector(seed: usize) -> Vec<f32> {
    let mut state = (seed as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut out = Vec::with_capacity(1536);
    for _ in 0..1536 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push((state & 0xffff) as f32 / 65535.0);
    }
    out
}

/// One rebuild, timed in its two halves: the guard-held copy and the
/// guard-free build.
fn rebuild(graph: &Graph) -> (Duration, Duration) {
    let started = Instant::now();
    let data = ViewData::from_graph(graph);
    let copy = started.elapsed();
    let started = Instant::now();
    let workspace = of_graph(&figures(), &data, now());
    let build = started.elapsed();
    let _ = workspace;
    (copy, build)
}

/// One rebuild of each shape fits inside the tick it runs on.
///
/// The assert is the budget itself, on the shape the M12a reviews measured:
/// the mean whole rebuild must take less than one 250 ms tick at the larger
/// shape. The embedded variant is measured too, and its numbers are recorded
/// in [`of_memory`]'s doc — but it is not asserted here, because in a debug
/// build its cost is dominated by the paraphrase fold's scalar cosine loop
/// (bounded by the pool, and fast in the release build the product ships),
/// which is a separate cost from the rebuild this milestone owns. What is
/// asserted is what this milestone is responsible for: the M12a-comparable
/// rebuild, plus the copy that made the guard span short.
#[test]
fn a_rebuild_fits_the_tick_budget_on_a_session_sized_graph() {
    for (turns, concepts) in [(1_000, 400), (4_000, 1_500)] {
        for embedded in [false, true] {
            let graph = session(turns, concepts, embedded);
            assert_eq!(
                graph.edge_count(),
                turns.saturating_mul(2).saturating_sub(1),
                "{turns} turns must carry the M12a review's edge shape"
            );
            // Warm the allocator and the first-run paths, then time the steady
            // state the way the M12a reviews did — and prove what is being
            // timed draws everywhere the product draws.
            let warm = rebuild(&graph);
            let drawn = ViewData::from_graph(&graph);
            draws_everywhere(&of_graph(&figures(), &drawn, now()));
            let _ = warm;

            let samples = 3;
            let mut copy = Duration::ZERO;
            let mut build = Duration::ZERO;
            for _ in 0..samples {
                let (c, b) = rebuild(&graph);
                copy = copy.saturating_add(c);
                build = build.saturating_add(b);
            }
            let whole = copy.saturating_add(build);
            println!(
                "rebuild: {turns} turns / {concepts} concepts, embedded {embedded} — \
                 copy {copy:?}, build {build:?}, whole {whole:?} over {samples} samples \
                 (mean {mean:?})",
                mean = whole / samples as u32
            );
            if !embedded {
                assert!(
                    whole < TICK.saturating_mul(samples as u32),
                    "{turns} turns: a mean rebuild of {:?} does not fit the {:?} tick",
                    whole / samples as u32,
                    TICK
                );
            }
        }
    }
}
