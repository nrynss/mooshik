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
    assert_eq!(
        names,
        vec![TOOL_RECALL, TOOL_DERIVE, TOOL_STATS, TOOL_SCRATCH]
    );
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
        serde_json::from_str(&tools.execute(TOOL_STATS, &json!({ "agent_id": "mooshik" })))
            .unwrap();
    tools.execute(
        TOOL_DERIVE,
        &json!({ "agent_id": "mooshik", "concepts": [{ "content": "stats marker", "concept_type": "observation" }] }),
    );
    let after: Value =
        serde_json::from_str(&tools.execute(TOOL_STATS, &json!({ "agent_id": "mooshik" })))
            .unwrap();
    let before_n = before["concept_count"].as_u64().unwrap();
    let after_n = after["concept_count"].as_u64().unwrap();
    assert_eq!(after["session"], "mooshik");
    assert!(
        after_n > before_n,
        "stats must observe derive: {before_n} -> {after_n}"
    );
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
    assert!(
        !out.starts_with('{'),
        "must not be a successful result: {out}"
    );
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
    assert!(
        out.starts_with(crate::text::get("tools.bad_param")),
        "{out}"
    );
}

#[tokio::test]
async fn out_of_range_top_k_is_refused() {
    let tools = MemoryTools::from_memory(fixture_memory().await);
    let out = tools.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": "x", "top_k": 500 }),
    );
    assert!(out.contains(crate::text::get("tools.range_error")), "{out}");
}

// --- run_scratch_script through the executor: permission seam + success ----

#[tokio::test]
async fn scratch_is_denied_when_confirmation_is_refused() {
    let tools = MemoryTools::from_memory(fixture_memory().await).with_scratch(ScratchConfig {
        confirm: Box::new(|_| false),
        max_output_bytes: 4096,
        secret_env: Vec::new(),
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
        secret_env: Vec::new(),
    });
    let out: Value = serde_json::from_str(&tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "echo hello from scratch" }),
    ))
    .unwrap();
    assert_eq!(out["exit_code"], 0);
    assert!(out["stdout"]
        .as_str()
        .unwrap()
        .contains("hello from scratch"));
    assert_eq!(out["timed_out"], false);
}

#[tokio::test]
async fn scratch_timeout_is_reported_by_the_executor() {
    let tools = MemoryTools::from_memory(fixture_memory().await).with_scratch(ScratchConfig {
        confirm: Box::new(|_| true),
        max_output_bytes: 4096,
        secret_env: Vec::new(),
    });
    let out: Value = serde_json::from_str(&tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "sleep 60", "timeout_secs": 1 }),
    ))
    .unwrap();
    assert_eq!(out["timed_out"], true);
}

// --- sync-path panic containment: a panic is an error string, not a crash ----

#[tokio::test]
async fn a_panicking_sync_tool_is_contained_as_an_error_string() {
    // P2-M4-3 pin: `execute` wraps `dispatch` in a `catch_unwind`, so a tool
    // that panics on the caller thread (here: a panicking `confirm` closure)
    // yields a contained generic error string — never a panic out of execute,
    // which would take down the chat loop as a dead process.
    let tools = MemoryTools::from_memory(fixture_memory().await).with_scratch(ScratchConfig {
        confirm: Box::new(|_| panic!("confirm exploded")),
        max_output_bytes: 4096,
        secret_env: Vec::new(),
    });
    let out = tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "echo hi" }),
    );
    assert_eq!(
        out,
        crate::text::get("tools.internal_error"),
        "the sync-path panic must be contained to a generic error string"
    );
}

// --- graceful degradation: chat still runs when memory cannot open ----------

#[test]
fn for_chat_returns_none_when_memory_cannot_open() {
    // Product default is Postgres with no DSN -> `MissingDsn` fails the open
    // fast (no network), so `for_chat` degrades to `None` instead of stalling.
    let tools = MemoryTools::for_chat(&Config::default(), None);
    assert!(tools.is_none());
}

// --- M5 composition pins: the ONE choke point is actually in place ----------

#[test]
fn executor_for_chat_composes_gate_then_redaction_then_tools() {
    // P1-M5-1 + M6 pin, same technique as the M3 graph-independence seams:
    // the production half of this module must compose the boundary in the
    // documented order — permission gate decides whether the call runs at
    // all, the inner executor runs, [`RedactingTools`] scans the final
    // result post-execute pre-history — and hand the whole stack out as an
    // Arc<dyn ToolExecutor>. A refactor that drops either wrap fails here.
    let production = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap();
    let factory = production
        .split("pub fn executor_for_chat")
        .nth(1)
        .expect("executor_for_chat must exist");
    assert!(
        factory.contains("RedactingTools::new(inner"),
        "executor_for_chat must wrap its inner executor (even the No-op \
         fallback) in RedactingTools; without the wrap tool results reach \
         the model unscanned"
    );
    assert!(
        factory.contains("GatedTools::new(redacting"),
        "the gate must sit in FRONT of redaction: permission first, \
         execute, redact"
    );
    assert!(
        factory.contains("Arc::new(GatedTools::new(redacting"),
        "the gated executor must be handed out as an Arc<dyn ToolExecutor>"
    );
}

#[test]
fn executor_for_chat_notices_when_the_vault_is_unavailable() {
    // M6 stance: a vault that cannot open never blocks chat; one stderr
    // notice explains that results are unredacted-only-because-unopenable.
    let production = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap();
    let factory = production
        .split("pub fn executor_for_chat")
        .nth(1)
        .expect("executor_for_chat must exist");
    assert!(
        factory.contains(r#"text::get("tools.vault_unavailable")"#),
        "a missing vault handle must produce the en.toml notice"
    );
}

#[test]
fn for_chat_holds_the_inner_scratch_seam_open_under_the_gate() {
    // P3-M5-5 pin: `for_chat` must set `ScratchConfig::always_confirmed()`
    // so a prompt-mode grant asks exactly once at the gate. Regressing to the
    // default seam would double-prompt (gate + inner) and nothing noticed.
    let production = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap();
    let for_chat = production
        .split("pub fn for_chat")
        .nth(1)
        .expect("for_chat must exist")
        .split("\npub fn ")
        .next()
        .unwrap();
    assert!(
        for_chat.contains("ScratchConfig::always_confirmed()"),
        "for_chat must hold the inner scratch confirm seam open so the gate \
         prompts exactly once"
    );
}

#[test]
fn executor_for_chat_gates_even_the_noop_fallback() {
    // Behavioral half of P1-M5-1: with the product-default store (Postgres,
    // no DSN) memory cannot open, so `executor_for_chat` falls back to the
    // Noop executor — and even that surface must come back gated. A denied
    // tool is refused by the gate before it ever reaches the inner executor;
    // an ungated composition would answer with `companion.unknown_tool`
    // instead of the permission refusal.
    let config = Config::from_toml_and_env("[permissions]\nscratch = 'deny'\n", []).unwrap();
    let executor = super::executor_for_chat(&config, None);
    assert!(
        executor.specs().is_empty(),
        "the Noop fallback advertises nothing"
    );
    assert_eq!(
        executor.execute(
            TOOL_SCRATCH,
            &json!({ "language": "bash", "code": "echo hi" }),
        ),
        crate::text::get("permissions.denied"),
        "a denied call through the fallback must be refused by the gate"
    );
    // A granted name still passes the gate and reaches the inner executor
    // (which answers unknown-tool, since there is no memory behind it).
    assert_eq!(
        executor.execute(
            TOOL_RECALL,
            &json!({ "agent_id": "mooshik", "query": "anything" }),
        ),
        crate::text::get("companion.unknown_tool"),
        "granted calls must pass through the gate to the inner executor"
    );
}

// --- M6: scratch secret injection + egress redaction ------------------------

use crate::vault::{PassphraseProvider, Vault};
use std::sync::Arc;

/// Open a throwaway vault preloaded with secrets for boundary tests.
fn fixture_vault(secrets: &[(&str, &str)]) -> crate::vault::SharedVault {
    let dir = std::env::temp_dir().join(format!(
        "mooshik-tools-vault-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut vault = Vault::open(
        dir.join("vault"),
        Arc::new(PassphraseProvider::new("pw").unwrap()),
    )
    .unwrap();
    for (name, value) in secrets {
        vault.set(name, value).unwrap();
    }
    vault.shared()
}

fn confirmed_scratch(secret_env: Vec<(String, String)>) -> ScratchConfig {
    ScratchConfig {
        confirm: Box::new(|_| true),
        max_output_bytes: 4096,
        secret_env,
    }
}

/// The full chat composition (gate → redactor → tools) over a fixture memory.
fn composed_stack(
    memory: Memory,
    vault: crate::vault::SharedVault,
    table: Vec<(String, String)>,
) -> Arc<dyn ToolExecutor> {
    let tools = MemoryTools::from_memory(memory)
        .with_vault(Some(vault.clone()))
        .with_scratch(confirmed_scratch(table));
    let config = Config::from_toml_and_env("[permissions]\nscratch = 'allow'\n", []).unwrap();
    Arc::new(GatedTools::new(
        Arc::new(RedactingTools::new(Arc::new(tools), Some(vault))),
        config.permissions.grants(),
    ))
}

#[tokio::test]
async fn scratch_echo_round_trips_injected_secret_to_redacted_output() {
    const VALUE: &str = "mooshik-live-secret-value";
    let vault = fixture_vault(&[("github-token", VALUE)]);
    let table = vec![("MOOSHIK_TEST_TOKEN".to_owned(), "github-token".to_owned())];
    let code = "printf '%s' \"$MOOSHIK_TEST_TOKEN\"";

    // Inner truth: without redaction the injected value comes straight back.
    let raw = MemoryTools::from_memory(fixture_memory().await)
        .with_vault(Some(vault.clone()))
        .with_scratch(confirmed_scratch(table.clone()));
    let inner: Value = serde_json::from_str(
        &raw.execute(TOOL_SCRATCH, &json!({ "language": "bash", "code": code })),
    )
    .unwrap();
    assert_eq!(inner["stdout"], VALUE, "injection must reach the child env");

    // The composed boundary redacts that same output before it crosses back.
    let stack = composed_stack(fixture_memory().await, vault, table);
    let out = stack.execute(TOOL_SCRATCH, &json!({ "language": "bash", "code": code }));
    assert!(
        !out.contains(VALUE),
        "the secret must never cross back: {out}"
    );
    assert!(out.contains("[REDACTED]"), "{out}");
}

#[tokio::test]
async fn every_configured_secret_is_redacted_from_tool_results() {
    let vault = fixture_vault(&[("alpha", "alpha-live"), ("beta", "beta-live")]);
    let table = vec![
        ("A".to_owned(), "alpha".to_owned()),
        ("B".to_owned(), "beta".to_owned()),
    ];
    let stack = composed_stack(fixture_memory().await, vault, table);
    let out = stack.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "printf '%s/%s' \"$A\" \"$B\"" }),
    );
    assert!(out.contains("[REDACTED]/[REDACTED]"), "{out}");
    assert!(
        !out.contains("alpha-live") && !out.contains("beta-live"),
        "{out}"
    );
}

#[tokio::test]
async fn derive_after_redaction_stores_only_the_marker() {
    // The M6 egress proof: a script echoing $TOKEN gets its result redacted
    // at the boundary; deriving from that content stores only `[REDACTED]`
    // text. The graph never sees the value.
    const VALUE: &str = "graph-never-sees-this";
    let vault = fixture_vault(&[("token", VALUE)]);
    let table = vec![("TOKEN".to_owned(), "token".to_owned())];
    let stack = composed_stack(fixture_memory().await, vault, table);

    let out: Value = serde_json::from_str(&stack.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "printf '%s' \"$TOKEN\"" }),
    ))
    .unwrap();
    assert_eq!(out["stdout"], "[REDACTED]");

    let content = format!("the script printed: {}", out["stdout"].as_str().unwrap());
    stack.execute(
        TOOL_DERIVE,
        &json!({
            "agent_id": "mooshik",
            "concepts": [{ "content": content, "concept_type": "observation" }],
        }),
    );
    let recalled = stack.execute(
        TOOL_RECALL,
        &json!({ "agent_id": "mooshik", "query": "the script printed" }),
    );
    assert!(recalled.contains("[REDACTED]"), "{recalled}");
    assert!(
        !recalled.contains(VALUE),
        "the graph saw the value: {recalled}"
    );
}

#[tokio::test]
async fn injection_resolves_per_run_so_rotation_is_observed() {
    let vault = fixture_vault(&[("rotating", "first-run-value")]);
    let table = vec![("TOKEN".to_owned(), "rotating".to_owned())];
    let tools = MemoryTools::from_memory(fixture_memory().await)
        .with_vault(Some(vault.clone()))
        .with_scratch(confirmed_scratch(table));
    let code = "printf '%s' \"$TOKEN\"";

    let first: Value = serde_json::from_str(
        &tools.execute(TOOL_SCRATCH, &json!({ "language": "bash", "code": code })),
    )
    .unwrap();
    assert_eq!(first["stdout"], "first-run-value");

    // Rotate the secret between runs; the next run must see the new value.
    crate::vault::lock_shared(&vault)
        .set("rotating", "second-run-value")
        .unwrap();
    let second: Value = serde_json::from_str(
        &tools.execute(TOOL_SCRATCH, &json!({ "language": "bash", "code": code })),
    )
    .unwrap();
    assert_eq!(second["stdout"], "second-run-value");
}

#[tokio::test]
async fn missing_secret_fails_the_script_before_it_starts() {
    const PRESENT: &str = "present-value";
    let vault = fixture_vault(&[("present", PRESENT)]);
    let table = vec![
        ("OK".to_owned(), "present".to_owned()),
        ("GAP".to_owned(), "absent-secret".to_owned()),
    ];
    let tools = MemoryTools::from_memory(fixture_memory().await)
        .with_vault(Some(vault))
        .with_scratch(confirmed_scratch(table));

    let out = tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "printf 'ran %s' \"$OK\"" }),
    );
    assert_eq!(
        out,
        crate::text::get("tools.scratch_secret_missing").replace("{name}", "absent-secret"),
        "the failure must be a contained error naming only the secret name"
    );
    assert!(!out.starts_with('{'), "no partial run result: {out}");
    assert!(
        !out.contains(PRESENT),
        "all-or-nothing: nothing may run half-injected"
    );
}

#[tokio::test]
async fn scratch_env_without_a_vault_fails_closed() {
    let tools = MemoryTools::from_memory(fixture_memory().await)
        .with_vault(None)
        .with_scratch(confirmed_scratch(vec![(
            "TOKEN".to_owned(),
            "whatever".to_owned(),
        )]));
    let out = tools.execute(
        TOOL_SCRATCH,
        &json!({ "language": "bash", "code": "echo hi" }),
    );
    assert_eq!(out, crate::text::get("tools.scratch_env_unavailable"));
}

#[test]
fn chat_composes_and_answers_even_when_the_vault_cannot_open() {
    // Behavioral half of the availability stance: `vault = None` still yields
    // a gated, answering executor — the No-op fallback advertises nothing and
    // granted calls pass the gate (here answered by the fallback itself).
    let executor = super::executor_for_chat(&Config::default(), None);
    assert!(executor.specs().is_empty());
    assert_eq!(
        executor.execute(
            TOOL_RECALL,
            &json!({ "agent_id": "mooshik", "query": "anything" }),
        ),
        crate::text::get("companion.unknown_tool"),
    );
}
