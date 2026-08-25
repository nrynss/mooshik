//! End-to-end tests for the M4 tool surface against a fixture `Memory`.
//!
//! Every test uses `StoreKind::Memory` + `EmbedderKind::Fixture` (dim 1024) so
//! default `cargo test` never touches a live model, network, or database — the
//! same fixture pattern as `src/memory/ops.rs`.

use lambo::{EmbedderKind, Memory, StoreKind};
use serde_json::{json, Value};

use super::*;
use crate::config::Config;

/// Provision and open an in-process fixture memory for the tool tests.
async fn fixture_memory() -> Memory {
    let mut config = Config::default();
    config.store.kind = StoreKind::Memory;
    config.embedder.kind = EmbedderKind::Fixture;
    config.embedder.dim = 1024;
    config.session.id = "mooshik".to_owned();
    crate::memory::provision(&config).await.unwrap();
    crate::memory::open(&config).await.unwrap()
}

#[tokio::test]
async fn specs_expose_the_four_in_scope_tools() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let specs = tools.specs();
    // The four in-scope tools, in a stable order (also exercised by the
    // Session tool loop, which iterates them for the prompt).
    let names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    assert_eq!(names, vec![TOOL_RECALL, TOOL_DERIVE, TOOL_STATS, TOOL_SCRATCH]);
    // Every tool's parameters open as a JSON object (root `$ref` inlined).
    for spec in &specs {
        assert_eq!(spec.parameters["type"], "object", "{}", spec.name);
        assert!(spec.parameters["properties"].is_object(), "{}", spec.name);
    }
}

#[tokio::test]
async fn derive_then_recall_round_trips() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    const MARKER: &str = "mooshik m4 recall round trip marker";

    let derived = tools.execute(
        TOOL_DERIVE,
        &json!({ "agent_id": "mooshik", "concepts": [{ "content": MARKER, "concept_type": "entity" }] }),
    );
    let created: Value = serde_json::from_str(&derived).unwrap();
    assert_eq!(created["created"].as_array().unwrap().len(), 1);

    let recalled = tools.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": MARKER }),
    );
    let result: Value = serde_json::from_str(&recalled).unwrap();
    assert!(
        !result["hits"].as_array().unwrap().is_empty(),
        "derive-then-recall must find the concept"
    );
}

#[tokio::test]
async fn derive_with_parent_of_creates_both_concepts() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let out = tools.execute(
        TOOL_DERIVE,
        &json!({
            "agent_id": "mooshik",
            "concepts": [
                { "content": "m4-animal", "concept_type": "entity" },
                { "content": "m4-cat", "concept_type": "entity" },
            ],
            "parent_of": [{ "parent": "m4-animal", "child": "m4-cat" }],
        }),
    );
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["created"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn stats_observes_derives() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let before: Value =
        serde_json::from_str(&tools.execute(TOOL_STATS, &json!({ "agent_id": "mooshik" }))).unwrap();
    tools.execute(
        TOOL_DERIVE,
        &json!({ "agent_id": "mooshik", "concepts": [{ "content": "stats marker", "concept_type": "observation" }] }),
    );
    let after: Value =
        serde_json::from_str(&tools.execute(TOOL_STATS, &json!({ "agent_id": "mooshik" }))).unwrap();
    let before_n = before["concept_count"].as_u64().unwrap();
    let after_n = after["concept_count"].as_u64().unwrap();
    assert_eq!(after["session"], "mooshik");
    assert!(after_n > before_n, "stats must observe derive: {before_n} -> {after_n}");
}

// --- bad-parameter discipline: refused as an error string, never a panic ----

#[tokio::test]
async fn unknown_field_is_refused_as_a_tool_error() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let out = tools.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": "x", "bogus": 1 }),
    );
    assert!(out.contains("unknown field"), "{out}");
    assert!(!out.starts_with('{'), "must not be a successful result: {out}");
}

#[tokio::test]
async fn over_length_query_is_refused() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let long = "x".repeat(schema::MAX_STRING_CHARS + 1);
    let out = tools.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": long }),
    );
    assert!(out.contains("must be at most"), "{out}");
}

#[tokio::test]
async fn wrong_type_knob_is_refused() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let out = tools.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": "x", "top_k": "many" }),
    );
    assert!(out.starts_with(&crate::text::get("tools.bad_param")), "{out}");
}

#[tokio::test]
async fn out_of_range_top_k_is_refused() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let out = tools.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": "x", "top_k": 500 }),
    );
    assert!(out.contains(&crate::text::get("tools.range_error")), "{out}");
}

// --- run_scratch_script through the executor: permission seam + success ----

#[tokio::test]
async fn scratch_is_denied_when_confirmation_is_refused() {
    let tools = MemoryTools::from_memory(fixture_memory().await).with_scratch(ScratchConfig {
        confirm: Box::new(|_| false),
        max_output_bytes: 4096,
    });
    let out = tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "echo hi" }),
    );
    assert_eq!(out, crate::text::get("tools.scratch_denied"));
}

#[tokio::test]
async fn scratch_runs_when_confirmed() {
    let tools = MemoryTools::from_memory(fixture_memory().await).with_scratch(ScratchConfig {
        confirm: Box::new(|_| true),
        max_output_bytes: 4096,
    });
    let out: Value = serde_json::from_str(
        &tools.execute(
            TOOL_SCRATCH,
            &json!({ "language": "bash", "code": "echo hello from scratch" }),
        ),
    )
    .unwrap();
    assert_eq!(out["exit_code"], 0);
    assert!(out["stdout"].as_str().unwrap().contains("hello from scratch"));
    assert_eq!(out["timed_out"], false);
}

#[tokio::test]
async fn scratch_timeout_is_reported_by_the_executor() {
    let tools = MemoryTools::from_memory(fixture_memory().await).with_scratch(ScratchConfig {
        confirm: Box::new(|_| true),
        max_output_bytes: 4096,
    });
    let out: Value = serde_json::from_str(&tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "sleep 60", "timeout_secs": 1 }),
    ))
    .unwrap();
    assert_eq!(out["timed_out"], true);
}

// --- graceful degradation: chat still runs when memory cannot open ----------

#[test]
fn for_chat_returns_none_when_memory_cannot_open() {
    // Product default is Postgres with no DSN -> `MissingDsn` fails the open
    // fast (no network), so `for_chat` degrades to `None` instead of stalling.
    let tools = MemoryTools::for_chat(&Config::default());
    assert!(tools.is_none());
}