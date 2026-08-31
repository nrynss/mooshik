//! Live-graph tests for the reflect milestone's observable contracts.
//!
//! The plan-layer tests in the sibling modules prove the pipeline asks the
//! right questions; these prove the **write path** answers them on a real
//! sqlite graph — the prose reflect writes round-trips through the view, the
//! paraphrase consolidation preserves losers and reroutes edges, and a re-run
//! is first-write-only. Each opens a real [`lambo::Memory`] over a temp
//! sqlite file (the M2/M8 `live_sqlite_…` pattern), because an in-memory store
//! serializes nothing and could not prove the mutations survive a reload.
use chrono::{DateTime, Utc};
use lambo::{
    graph::derive::ParentOf, AgentId, CanonizationStatus, Concept, ConceptType, Edge, EdgeType,
    Interaction, NodeId, SessionId,
};

use super::{
    apply_cluster, plan_paraphrase_consolidation, record_cluster_action, run_reflect,
    FixtureReflector, GraphSnapshot, ProseIndex,
};

/// A config over a fresh temp sqlite file with the fixture embedder.
fn sqlite_config(label: &str) -> (std::path::PathBuf, crate::config::Config) {
    let home = crate::secure_path::canonical_temp_dir() // ensure_dir
        .join(format!("mooshik-reflect-{label}-{}", std::process::id()));
    // canonical_temp_dir may not exist yet; create it.
    let _ = std::fs::create_dir_all(&home);
    let mut config = crate::config::Config::default();
    config.store.kind = lambo::StoreKind::Sqlite;
    config.store.path = Some(home.join("graph.db").to_string_lossy().into_owned());
    config.embedder.kind = lambo::EmbedderKind::Fixture;
    config.embedder.dim = 1024;
    config.session.id = "mooshik".to_owned();
    config.session.agent = "mooshik".to_owned();
    (home, config)
}

/// R1-2 — the pane surfaces the prose reflect wrote.
///
/// Prose written through the fixture reflector into a live sqlite graph must
/// reach [`crate::memory::view::of_memory`]: a day's mood/notes/highlights and
/// a thread's because. This is the milestone's "on screen, written by
/// reflect" contract, and it is what makes the prose read path (`ProseIndex`,
/// `prose_for_day`, `reason_for_thread`) more than dead code.
#[tokio::test]
async fn the_view_surfaces_the_prose_reflect_wrote() {
    let (home, config) = sqlite_config("view");
    crate::memory::provision(&config).await.unwrap();
    let memory = crate::memory::open(&config).await.unwrap();

    // A thread: two turns reaching the same thought.
    for _ in 0..2 {
        memory
            .derive(
                &[("block, never drop", ConceptType::Entity)],
                &ParentOf::none(),
            )
            .await
            .unwrap();
    }
    let outcome = run_reflect(&memory, &FixtureReflector, false, Utc::now())
        .await
        .unwrap();
    assert!(
        outcome.prose_count() > 0,
        "a reflect run over a day with turns must propose prose"
    );
    memory.close().await.unwrap();

    // Reopened from disk — the prose survives the store.
    let memory = crate::memory::open(&config).await.unwrap();
    let workspace = crate::memory::view::of_memory(&memory, chrono::Local::now());
    memory.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);

    assert!(
        workspace.today.mood.is_some(),
        "reflect's mood did not reach today's pane"
    );
    assert!(
        !workspace.today.notes.is_empty(),
        "reflect's notes did not reach today's pane"
    );
    assert!(
        !workspace.today.highlights.is_empty(),
        "reflect's gutter summary did not reach today's pane"
    );
    assert!(
        !workspace.threads.is_empty() && !workspace.threads[0].because.is_empty(),
        "reflect's thread reason did not reach the threads pane"
    );
}

/// R2 regression guard — an empty graph still yields the empty defaults, so
/// the prose wiring changed nothing about a graph nobody has reflected on.
#[tokio::test]
async fn an_unreflected_graph_keeps_the_empty_prose_defaults() {
    let (home, config) = sqlite_config("empty");
    crate::memory::provision(&config).await.unwrap();
    let memory = crate::memory::open(&config).await.unwrap();
    memory
        .derive(
            &[("a single thought", ConceptType::Entity)],
            &ParentOf::none(),
        )
        .await
        .unwrap();
    let workspace = crate::memory::view::of_memory(&memory, chrono::Local::now());
    memory.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);

    assert!(workspace.today.mood.is_none(), "no prose means no mood");
    assert!(
        workspace.today.highlights.is_empty(),
        "no prose means no gutter"
    );
    assert!(workspace.today.notes.is_empty(), "no prose means no notes");
    assert!(
        workspace
            .threads
            .iter()
            .all(|thread| thread.because.is_empty()),
        "no prose means no thread reason"
    );
}

/// R7 — prose is first-write-only and a re-run adds no duplicate.
///
/// Re-running reflect on a day that already has prose must plan nothing for
/// that day (the documented "one prose concept per day / per thread"
/// contract), so the graph holds exactly the first run's prose, once each.
#[tokio::test]
async fn re_running_reflect_is_first_write_only() {
    let (home, config) = sqlite_config("first-write");
    crate::memory::provision(&config).await.unwrap();
    let memory = crate::memory::open(&config).await.unwrap();
    memory
        .derive(
            &[("distinct thought", ConceptType::Entity)],
            &ParentOf::none(),
        )
        .await
        .unwrap();

    let first = run_reflect(&memory, &FixtureReflector, false, Utc::now())
        .await
        .unwrap();
    assert!(first.prose_count() > 0, "the first run writes prose");

    let second = run_reflect(&memory, &FixtureReflector, false, Utc::now())
        .await
        .unwrap();
    assert_eq!(
        second.prose_count(),
        0,
        "a re-run must not re-plan prose for a day that already has it"
    );
    assert_eq!(
        second.paraphrase_merges.len(),
        0,
        "a re-run must not plan expression merges on a single-concept graph"
    );
    memory.close().await.unwrap();

    // Exactly the first run's prose survives, once each.
    let memory = crate::memory::open(&config).await.unwrap();
    let snapshot = {
        let g = memory.graph().read();
        GraphSnapshot::from_graph(&g)
    };
    let index = ProseIndex::from_snapshot(&snapshot);
    assert_eq!(
        index.len(),
        first.prose_count(),
        "a re-run must not have duplicated any prose concept"
    );
    assert!(!index.is_empty());
    memory.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// R5 — the consolidation write path, against a real sqlite graph.
///
/// Builds two paraphrase twins (identical embeddings below [`super::super::view::PARAPHRASE`]),
/// one returned by two turns and one by one, and applies the single cluster.
/// Pins the five mutation points the round-1 review found untested: loser
/// content survives verbatim, the survivor's Derives in-count absorbs the
/// loser's, rerouting moves a *real* reroutable edge (the loser's own return
/// from a third interaction) to the survivor, the strongest survives, and a
/// re-plan after the apply is a true no-op.
#[tokio::test]
async fn consolidation_write_path_preserves_losers_absorbs_derives_and_is_a_no_op() {
    let (home, config) = sqlite_config("write-path");
    crate::memory::provision(&config).await.unwrap();
    let memory = crate::memory::open(&config).await.unwrap();
    let agent = memory.agent().clone();

    let survivor_id;
    let loser_id;
    let second_turn;
    let third_turn;
    let emb = vec![0.25f32; 1024];
    {
        let mut g = memory.graph().write();
        let i1 = NodeId::new();
        let i2 = NodeId::new();
        let i3 = NodeId::new();
        let session = memory.session().clone();
        let base = Utc::now();
        let mk = |id: NodeId, previous_id: Option<NodeId>, created_at: DateTime<Utc>| Interaction {
            id,
            session_id: session.clone(),
            agent_id: agent.clone(),
            prompt_text: None,
            previous_id,
            created_at,
            event_time: None,
        };
        // Three turns: the origin, the survivor's second return (i2), and —
        // at a timestamp *between* the two — the loser's own return (i3).
        // Ordering matters twice: the loser's reroutable edge comes from i3,
        // so the reroute loop has real work to do; and because the survivor's
        // latest source (i2) stays strictly later than the loser's (i3),
        // `strongest_first` keeps the survivor on top once both concepts
        // carry two returns.
        g.insert_interaction(mk(i1, None, base)).unwrap();
        g.insert_interaction(mk(i3, Some(i1), base + chrono::Duration::seconds(10)))
            .unwrap();
        g.insert_interaction(mk(i2, Some(i3), base + chrono::Duration::seconds(20)))
            .unwrap();
        second_turn = i2;
        third_turn = i3;

        survivor_id = NodeId::new();
        loser_id = NodeId::new();
        // Two twin concepts: identical embedding makes them one cluster; the
        // survivor is reached by both turns, the loser by one.
        g.insert_concept(
            concept(
                survivor_id,
                "the ring holds 512",
                "ring-512",
                i1,
                emb.clone(),
            ),
            i1,
        )
        .unwrap();
        g.insert_concept(
            concept(
                loser_id,
                "the ring caps at 512 copies",
                "ring-cap",
                i1,
                emb.clone(),
            ),
            i1,
        )
        .unwrap();
        // The loser's return from a *third* interaction: this is the edge the
        // reroute loop must move to the survivor — not its origin (which
        // `insert_concept` recreates) and not the survivor's own source.
        let e = edge(&session, i3, loser_id, base + chrono::Duration::seconds(10));
        g.upsert_edge(e).unwrap();
        // Second return to the survivor.
        let e = edge(
            &session,
            i2,
            survivor_id,
            base + chrono::Duration::seconds(20),
        );
        g.upsert_edge(e).unwrap();
    }

    let mut snap = {
        let g = memory.graph().read();
        GraphSnapshot::from_graph(&g)
    };
    let plan = plan_paraphrase_consolidation(&snap);
    assert_eq!(plan.clusters.len(), 1, "the twins must form one cluster");
    let cluster = &plan.clusters[0];
    assert_eq!(
        cluster.survivor, survivor_id,
        "(d) the strongest concept must survive"
    );
    assert!(cluster.losers.contains(&loser_id));

    let survivor_content = snap.content_of(survivor_id).unwrap().to_owned();
    {
        let mut g = memory.graph().write();
        apply_cluster(&mut g, cluster, &mut snap);
    }
    record_cluster_action(&memory, &agent, cluster, &survivor_content).unwrap();

    // Re-planning against the updated snapshot must be a true no-op.
    assert!(
        plan_paraphrase_consolidation(&snap).clusters.is_empty(),
        "(e) re-planning after the apply must be empty — the loser is marked merged"
    );

    memory.close().await.unwrap();

    // Reopened from disk, every write-path claim must hold off the store.
    let memory = crate::memory::open(&config).await.unwrap();
    {
        let read = memory.graph().read();
        let loser_content = match read.node(loser_id) {
            Some(lambo::Node::Concept(c)) => c.content.clone(),
            _ => panic!("the loser concept must still exist"),
        };
        assert_eq!(
            loser_content,
            format!("[merged into {survivor_content}]: the ring caps at 512 copies"),
            "(a) the loser's original content must survive verbatim in the marker"
        );
        // (c) rerouting complete: the loser's *only* incoming Derives edge is the
        // structural one from its own origin interaction — every turn that reached
        // it now reaches the survivor, and nothing else points at it.
        let loser_origin = match read.node(loser_id) {
            Some(lambo::Node::Concept(c)) => c.origin_interaction,
            _ => unreachable!("the loser concept must still exist"),
        };
        assert_eq!(
            read.in_neighbors_typed(loser_id, EdgeType::Derives),
            vec![loser_origin],
            "(c) rerouting must be complete — only the loser's own origin edge remains"
        );
        let survivor_sources = read.in_neighbors_typed(survivor_id, EdgeType::Derives);
        assert_eq!(
            survivor_sources.len(),
            3,
            "(b) the survivor absorbs the loser's derives — origin, its own second turn, and the rerouted third"
        );
        // The rerouted edge is the loser's own return from i3; in id order it
        // must be exactly {origin, second turn, third turn}.
        let mut expected = vec![loser_origin, second_turn, third_turn];
        let mut got = survivor_sources.clone();
        expected.sort_by_key(|id| id.0);
        got.sort_by_key(|id| id.0);
        assert_eq!(
            got, expected,
            "(b) the loser's reroutable edge reaches the survivor"
        );
    }
    memory.close().await.unwrap();
    let _ = std::fs::remove_dir_all(&home);
}

/// A concept with an embedding, so the paraphrase walk can cluster it.
fn concept(id: NodeId, content: &str, key: &str, origin: NodeId, embedding: Vec<f32>) -> Concept {
    Concept {
        id,
        session_id: SessionId::new("mooshik"),
        content: content.to_owned(),
        canonical_key: key.to_owned(),
        concept_type: ConceptType::Entity,
        origin_interaction: origin,
        origin_agent: AgentId::new("mooshik"),
        created_at: Utc::now(),
        access_count: 0,
        last_accessed: None,
        gc_survived: 0,
        human_confirmed: 0,
        canonization_status: CanonizationStatus::None,
        blast_radius: None,
        last_demotion_time: None,
        embedding: Some(embedding),
        chunk_group_id: None,
    }
}

/// A `Derives` edge from a source interaction into a target thought.
fn edge(session: &SessionId, source: NodeId, target: NodeId, at: DateTime<Utc>) -> Edge {
    Edge {
        id: NodeId::new(),
        session_id: session.clone(),
        source,
        target,
        edge_type: EdgeType::Derives,
        weight: 1.0,
        reinforcements: 1,
        created_at: at,
        last_reinforced: at,
        event_time: None,
    }
}
