//! M4 — the tool surface: the four in-scope tools behind the companion seam.
//!
//! `lambo_recall`, `lambo_derive`, `lambo_stats` and `run_scratch_script` are
//! exposed through [`crate::companion::ToolExecutor`], backed by an in-process
//! [`lambo::Memory`] (not JSON-RPC). The lambo parameters are lifted verbatim
//! (see [`schema`]) and dispatched with the same panic-containment and
//! fail-closed discipline lambo applies at its own MCP boundary: a tool that
//! panics, or a parameter that violates a cap, returns an error *string*, never a
//! dead process or a poisoned chat loop.
//!
//! **Async seam.** `execute` is synchronous, but `Memory::recall`/`derive` are
//! async. The [`worker::ToolRuntime`] pins one Tokio runtime to a dedicated OS
//! thread and `execute` blocks on a bounded wait per call (a turn boundary,
//! mirroring the session's `tool_loop` cap). See [`worker`] for the design.

use lambo::{
    graph::derive::{DeriveOutcome, ParentOf},
    ConceptType, LamboError, Memory, RecallQuery,
};
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

use crate::companion::{NoopExecutor, ToolExecutor, ToolSpec};
use crate::config::Config;
use crate::text;

mod schema;
mod scratch;
mod worker;

pub use scratch::ScratchConfig;

use schema::{
    check_size, DeriveParams, RecallParams, ScratchParams, StatsParams, WireConceptType,
    MAX_CONCEPTS_PER_DERIVE, MAX_MAX_TOKENS, MAX_TOP_K, MAX_TRAVERSAL_DEPTH,
    SCRATCH_DEFAULT_TIMEOUT_SECS,
};
use worker::{ToolRunError, ToolRuntime};

pub const TOOL_RECALL: &str = "lambo_recall";
pub const TOOL_DERIVE: &str = "lambo_derive";
pub const TOOL_STATS: &str = "lambo_stats";
pub const TOOL_SCRATCH: &str = "run_scratch_script";

/// Bound every in-process lambo call (recall/derive) to a hard wait. Tool
/// execution is a turn boundary; this keeps a stalled memory from hanging chat.
const LAMBO_CALL_WAIT: Duration = Duration::from_secs(60);
/// Bound on opening Memory for a chat session.
const OPEN_WAIT: Duration = Duration::from_secs(20);

/// The in-scope tool specifications, exposed to the companion model.
pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: TOOL_RECALL.into(),
            description: text::get("tools.lambo_recall_desc").into(),
            parameters: schema::tool_parameters::<RecallParams>(),
        },
        ToolSpec {
            name: TOOL_DERIVE.into(),
            description: text::get("tools.lambo_derive_desc").into(),
            parameters: schema::tool_parameters::<DeriveParams>(),
        },
        ToolSpec {
            name: TOOL_STATS.into(),
            description: text::get("tools.lambo_stats_desc").into(),
            parameters: schema::tool_parameters::<StatsParams>(),
        },
        ToolSpec {
            name: TOOL_SCRATCH.into(),
            description: text::get("tools.run_scratch_script_desc").into(),
            parameters: schema::tool_parameters::<ScratchParams>(),
        },
    ]
}

/// A [`ToolExecutor`] that dispatches the four in-scope tools to an in-process
/// lambo [`Memory`].
pub struct MemoryTools {
    mem: Arc<Memory>,
    worker: ToolRuntime,
    scratch: ScratchConfig,
    /// The runtime that opened `mem`. Lambo spawns its flush daemon on the
    /// runtime current at `Memory::builder().build()`; dropping that runtime
    /// kills the daemon and nothing ever persists. Kept alive here so
    /// [`Drop::drop`] drives a graceful close on the same runtime.
    owner: Option<tokio::runtime::Runtime>,
}

impl Drop for MemoryTools {
    /// Flush the write-behind log and release the session lease on the same
    /// runtime that spawned the daemon. Without this a chat exiting on EOF or
    /// Ctrl-C drops the `Arc<Memory>` silently and every unflushed derive is
    /// lost with the process — the next process recalls nothing.
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            let mem = Arc::clone(&self.mem);
            owner.block_on(async move {
                if let Err(error) = mem.close().await {
                    eprintln!("memory close: {error}");
                }
            });
        }
    }
}

impl MemoryTools {
    pub fn for_chat(config: &Config) -> Option<Arc<dyn ToolExecutor>> {
        let opened = catch_unwind(AssertUnwindSafe(|| open_memory(config))).unwrap_or(None);
        opened.map(|(owner, memory)| {
            Arc::new(MemoryTools {
                mem: Arc::new(memory),
                worker: ToolRuntime::new(),
                scratch: ScratchConfig::default(),
                owner: Some(owner),
            }) as Arc<dyn ToolExecutor>
        })
    }

    /// Build an executor over an already-open `Memory` (used by tests with a
    /// fixture memory, and by callers that own their own open). The caller
    /// keeps responsibility for closing that memory.
    pub fn from_memory(memory: Memory) -> Self {
        Self {
            mem: Arc::new(memory),
            worker: ToolRuntime::new(),
            scratch: ScratchConfig::default(),
            owner: None,
        }
    }

    /// Override the scratch permission/cap configuration (tests and the M5 gate).
    pub fn with_scratch(mut self, scratch: ScratchConfig) -> Self {
        self.scratch = scratch;
        self
    }
    fn dispatch(&self, name: &str, arguments: &Value) -> String {
        match name {
            TOOL_RECALL => self.run_recall(arguments),
            TOOL_DERIVE => self.run_derive(arguments),
            TOOL_STATS => self.run_stats(arguments),
            TOOL_SCRATCH => self.run_scratch(arguments),
            _ => text::get("companion.unknown_tool").to_owned(),
        }
    }

    fn run_recall(&self, arguments: &Value) -> String {
        let params: RecallParams = match serde_json::from_value(arguments.clone()) {
            Ok(params) => params,
            Err(error) => return bad_param(&error),
        };
        if params.query.trim().is_empty() {
            return text::get("tools.query_empty").to_owned();
        }
        for field in [("agent_id", &params.agent_id), ("query", &params.query)] {
            if let Err(message) = check_size(field.0, field.1) {
                return message;
            }
        }
        let top_k = ranged(
            params.top_k,
            1..=MAX_TOP_K,
            self.mem.config().default_top_k,
            "top_k",
        );
        let max_tokens = ranged(
            params.max_tokens,
            1..=MAX_MAX_TOKENS,
            self.mem.config().default_max_tokens,
            "max_tokens",
        );
        let traversal_depth = ranged(
            params.traversal_depth,
            0..=MAX_TRAVERSAL_DEPTH,
            self.mem.config().default_traversal_depth,
            "traversal_depth",
        );
        let (top_k, max_tokens, traversal_depth) = match (top_k, max_tokens, traversal_depth) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            _ => return text::get("tools.range_error").to_owned(),
        };

        let query = RecallQuery {
            query: params.query,
            top_k,
            max_tokens,
            traversal_depth,
        };
        let memory = self.mem.clone();
        let result = self.worker.run(
            move |rt| rt.block_on(async move { memory.recall(query).await }),
            LAMBO_CALL_WAIT,
        );
        match result {
            Ok(Ok(recall)) => {
                serde_json::to_string(&recall).unwrap_or_else(|_| tool_internal_error())
            }
            Ok(Err(error)) => self.lambo_err(TOOL_RECALL, error),
            Err(run) => self.lambo_run_err(TOOL_RECALL, run),
        }
    }

    fn run_derive(&self, arguments: &Value) -> String {
        let params: DeriveParams = match serde_json::from_value(arguments.clone()) {
            Ok(params) => params,
            Err(error) => return bad_param(&error),
        };
        if params.concepts.is_empty() {
            return text::get("tools.derive_no_concepts").to_owned();
        }
        if params.concepts.len() > MAX_CONCEPTS_PER_DERIVE {
            return format!("concepts must contain at most {MAX_CONCEPTS_PER_DERIVE} entries");
        }
        if let Err(message) = check_size("agent_id", &params.agent_id) {
            return message;
        }
        for concept in &params.concepts {
            if concept.content.trim().is_empty() {
                return text::get("tools.derive_empty_concept").to_owned();
            }
            if let Err(message) = check_size("concept.content", &concept.content) {
                return message;
            }
        }
        if let Some(pairs) = &params.parent_of {
            for pair in pairs {
                if pair.parent.trim().is_empty() || pair.child.trim().is_empty() {
                    return text::get("tools.derive_empty_parent").to_owned();
                }
                if let Err(message) = check_size("parent_of.parent", &pair.parent) {
                    return message;
                }
                if let Err(message) = check_size("parent_of.child", &pair.child) {
                    return message;
                }
            }
        }

        // Everything below is moved (owned) into the worker-only `move` closure;
        // the borrowed `&[(&str, ConceptType)]` / `ParentOf` are rebuilt *inside*
        // the closure body so the closure stays a plain 'static-owned type (a
        // self-referential capture across the `move` boundary would not compile).
        let contents: Vec<String> = params
            .concepts
            .iter()
            .map(|concept| concept.content.clone())
            .collect();
        let kinds: Vec<ConceptType> = params
            .concepts
            .iter()
            .map(|concept| to_concept_type(concept.concept_type))
            .collect();
        let parent_pairs: Vec<(String, String)> = params
            .parent_of
            .iter()
            .flatten()
            .map(|pair| (pair.parent.clone(), pair.child.clone()))
            .collect();

        let memory = self.mem.clone();
        let result = self.worker.run(
            move |rt| {
                let concepts: Vec<(&str, ConceptType)> = contents
                    .iter()
                    .zip(kinds.iter().copied())
                    .map(|(content, kind)| (content.as_str(), kind))
                    .collect();
                let pairs: Vec<(&str, &str)> = parent_pairs
                    .iter()
                    .map(|(parent, child)| (parent.as_str(), child.as_str()))
                    .collect();
                let parent_of = if pairs.is_empty() {
                    ParentOf::none()
                } else {
                    ParentOf::from_pairs(&pairs)
                };
                rt.block_on(async move { memory.derive(&concepts, &parent_of).await })
            },
            LAMBO_CALL_WAIT,
        );
        match result {
            Ok(Ok(outcome)) => render_derive(&outcome),
            Ok(Err(error)) => self.lambo_err(TOOL_DERIVE, error),
            Err(run) => self.lambo_run_err(TOOL_DERIVE, run),
        }
    }

    fn run_stats(&self, arguments: &Value) -> String {
        let params: StatsParams = match serde_json::from_value(arguments.clone()) {
            Ok(params) => params,
            Err(error) => return bad_param(&error),
        };
        if let Err(message) = check_size("agent_id", &params.agent_id) {
            return message;
        }
        if let Some(receipt) = &params.receipt {
            if let Err(message) = check_size("receipt", receipt) {
                return message;
            }
        }
        let stats = self.mem.stats();
        let mut payload = json!({
            "session": stats.session.0,
            "agent": stats.agent.0,
            "flush_lag_ms": stats.flush_lag.as_millis() as u64,
            "log_depth": stats.log_depth,
            "flush_depth": stats.flush_depth,
            "dead_lettered": stats.dead_lettered,
            "degraded": stats.degraded,
            "node_count": stats.node_count,
            "edge_count": stats.edge_count,
            "concept_count": stats.concept_count,
            "embedded_concepts": stats.embedded_concepts,
            "canonical_count": stats.canonical_count,
            "epoch": stats.epoch,
            "daemon_cycles": stats.daemon_cycles,
            "canonization_cycles": stats.canonization_cycles,
            "canonization_failures": stats.canonization_failures,
        });
        // M4 uses lambo's synchronous `derive`, which never issues an async
        // receipt; `receipt`/`wait_ms` are accepted for schema compatibility and
        // reported as a no-op rather than silently ignored.
        if let Some(receipt) = &params.receipt {
            if let Some(object) = payload.as_object_mut() {
                object.insert(
                    "receipt".into(),
                    json!({ "id": receipt, "state": "never-issued" }),
                );
            }
        }
        payload.to_string()
    }

    fn run_scratch(&self, arguments: &Value) -> String {
        let params: ScratchParams = match serde_json::from_value(arguments.clone()) {
            Ok(params) => params,
            Err(error) => return bad_param(&error),
        };
        if let Err(message) = scratch::validate_scratch(&params) {
            return message;
        }
        if !(self.scratch.confirm)(&params) {
            return text::get("tools.scratch_denied").to_owned();
        }
        let timeout =
            Duration::from_secs(params.timeout_secs.unwrap_or(SCRATCH_DEFAULT_TIMEOUT_SECS));
        match scratch::run_script(
            &params.code,
            params.language,
            timeout,
            self.scratch.max_output_bytes,
        ) {
            Ok(outcome) => json!({
                "exit_code": outcome.exit_code,
                "stdout": outcome.stdout,
                "stderr": outcome.stderr,
                "truncated": outcome.truncated,
                "timed_out": outcome.timed_out,
                "duration_ms": outcome.duration.as_millis() as u64,
            })
            .to_string(),
            Err(message) => message,
        }
    }

    fn lambo_err(&self, what: &str, error: LamboError) -> String {
        eprintln!("{what}: memory error: {error}");
        format!("{what}: memory error (the detail was logged)")
    }

    fn lambo_run_err(&self, what: &str, run: ToolRunError) -> String {
        match run {
            ToolRunError::TimedOut => format!("{what}: {}", text::get("tools.tool_timeout")),
            ToolRunError::Panicked | ToolRunError::Unavailable => {
                eprintln!("{what}: tool runtime unavailable or panicked");
                tool_internal_error()
            }
        }
    }
}

impl ToolExecutor for MemoryTools {
    fn specs(&self) -> Vec<ToolSpec> {
        tool_specs()
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        match catch_unwind(AssertUnwindSafe(|| self.dispatch(name, arguments))) {
            Ok(output) => output,
            Err(payload) => {
                eprintln!("tool {name} panicked: {}", panic_message(&payload));
                tool_internal_error()
            }
        }
    }
}

/// The CLI-facing factory: open Memory for chat and hand back an executor that
/// degrades to a No-op when memory is unavailable (chat must still run). Lives
/// here, not in `cli`, so `cli::chat` stays free of any `crate::memory`/`provision`
/// reference (M3 pins).
pub fn executor_for_chat(config: &Config) -> Arc<dyn ToolExecutor> {
    match MemoryTools::for_chat(config) {
        Some(tools) => tools,
        None => {
            eprintln!("{}", text::get("tools.chat_memory_unavailable"));
            Arc::new(NoopExecutor)
        }
    }
}

fn open_memory(config: &Config) -> Option<(tokio::runtime::Runtime, Memory)> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .ok()?;
    let config = config.clone();
    let opened = runtime.block_on(async move {
        tokio::time::timeout(OPEN_WAIT, crate::memory::open(&config)).await
    });
    match opened {
        Ok(Ok(memory)) => Some((runtime, memory)),
        _ => None,
    }
}

fn render_derive(outcome: &DeriveOutcome) -> String {
    json!({
        "created": node_ids(&outcome.created),
        "matched": node_ids(&outcome.matched),
        "semantic_merged": node_ids(&outcome.semantic_merged),
        "reinforced": outcome.reinforced,
        "embedded": outcome.embedded,
    })
    .to_string()
}

fn node_ids(ids: &[lambo::NodeId]) -> Vec<String> {
    ids.iter().map(|id| id.0.to_string()).collect()
}

fn to_concept_type(kind: WireConceptType) -> ConceptType {
    match kind {
        WireConceptType::Entity => ConceptType::Entity,
        WireConceptType::Logic => ConceptType::Logic,
        WireConceptType::Constraint => ConceptType::Constraint,
        WireConceptType::Resource => ConceptType::Resource,
        WireConceptType::Observation => ConceptType::Observation,
    }
}

fn ranged(
    value: Option<usize>,
    range: std::ops::RangeInclusive<usize>,
    config_default: usize,
    _name: &str,
) -> Result<usize, ()> {
    match value {
        Some(v) if range.contains(&v) => Ok(v),
        Some(_) => Err(()),
        None => Ok(config_default.clamp(*range.start(), *range.end())),
    }
}

fn bad_param(error: &dyn std::fmt::Display) -> String {
    format!("{}: {error}", text::get("tools.bad_param"))
}

fn tool_internal_error() -> String {
    text::get("tools.internal_error").to_owned()
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    match payload.downcast_ref::<&str>() {
        Some(message) => (*message).to_owned(),
        None => match payload.downcast_ref::<String>() {
            Some(message) => message.clone(),
            None => "unknown panic".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests;
