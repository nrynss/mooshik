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

use crate::companion::{NoopExecutor, RecallInjector, ToolExecutor, ToolSpec};
use crate::config::{Config, Grants};
use crate::memory::WriteLane;
use crate::text;
use crate::vault::SharedVault;

mod diagnostics;
mod permissions;
mod recall;
pub mod redact;
mod schema;
mod scratch;
mod worker;

pub use diagnostics::Diagnostics;
pub use permissions::{Confirm, GatedTools};
pub use redact::RedactingTools;
pub use scratch::ScratchConfig;
pub use worker::{ToolRunError, ToolRuntime};

use schema::{
    check_size, DeriveParams, RecallParams, ScratchParams, StatsParams, WireConceptType,
    MAX_CONCEPTS_PER_DERIVE, MAX_MAX_TOKENS, MAX_TOP_K, MAX_TRAVERSAL_DEPTH,
    SCRATCH_DEFAULT_TIMEOUT_SECS,
};

pub const TOOL_RECALL: &str = "lambo_recall";
pub const TOOL_DERIVE: &str = "lambo_derive";
pub const TOOL_STATS: &str = "lambo_stats";
pub const TOOL_SCRATCH: &str = "run_scratch_script";

/// Bound every in-process lambo call (recall/derive) to a hard wait. Tool
/// execution is a turn boundary; this keeps a stalled memory from hanging chat.
const LAMBO_CALL_WAIT: Duration = Duration::from_secs(60);
/// Bound on opening Memory for a chat session.
const OPEN_WAIT: Duration = Duration::from_secs(20);

/// A [`ToolExecutor`] that combines a list of peer tool executors, dispatching
/// each call to whichever owns the tool name. M10 composes the memory tools
/// and the MCP host behind a single composite sibling so the *one* permission
/// gate ([`GatedTools`]) and the *one* egress redactor ([`RedactingTools`])
/// wrap BOTH — `mcp.*` grants flow through the same M5 path and MCP results
/// cross the same M6 scan.
pub struct CompositeTools {
    inner: Arc<dyn ToolExecutor>,
    mcp: Arc<dyn ToolExecutor>,
}

impl CompositeTools {
    pub fn new(inner: Arc<dyn ToolExecutor>, mcp: Arc<dyn ToolExecutor>) -> Self {
        Self { inner, mcp }
    }
}

impl ToolExecutor for CompositeTools {
    fn specs(&self) -> Vec<ToolSpec> {
        let mut all = self.inner.specs();
        all.extend(self.mcp.specs());
        all
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        if name.starts_with("mcp.") {
            self.mcp.execute(name, arguments)
        } else {
            self.inner.execute(name, arguments)
        }
    }
}

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
    /// The shared vault handle (chat), if the vault opened. `None` means
    /// chat runs unredacted-only-because-unopenable; scratch injection then
    /// fails closed per run.
    vault: Option<SharedVault>,
    /// The runtime that opened `mem`. Lambo spawns its flush daemon on the
    /// runtime current at `Memory::builder().build()`; dropping that runtime
    /// kills the daemon and nothing ever persists. Kept alive here so
    /// [`Drop::drop`] drives a graceful close on the same runtime.
    owner: Option<tokio::runtime::Runtime>,
    /// Held across every `derive` this executor performs, so a caller sharing
    /// `mem` with a second writer can hand both the same lane. See
    /// [`WriteLane`] for why lambo does not do this itself. On the chat path
    /// there is only one writer and the lane is never contended.
    writes: WriteLane,
    /// Execute-time diagnostics. Stderr on the CLI path; a channel on the pane.
    diagnostics: Diagnostics,
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
                if mem.close().await.is_err() {
                    eprintln!("{}", text::get("tools.close_failed"));
                }
            });
        }
    }
}

impl MemoryTools {
    pub fn for_chat(config: &Config, vault: Option<SharedVault>) -> Option<Arc<dyn ToolExecutor>> {
        let opened = catch_unwind(AssertUnwindSafe(|| open_memory(config))).unwrap_or(None);
        opened.map(|(owner, memory)| {
            Arc::new(MemoryTools {
                mem: Arc::new(memory),
                worker: ToolRuntime::new(),
                scratch: Self::chat_scratch(config),
                owner: Some(owner),
                vault,
                writes: WriteLane::new(),
                diagnostics: Diagnostics::stderr(),
            }) as Arc<dyn ToolExecutor>
        })
    }

    /// The scratch seam every *chat-shaped* surface installs — the CLI's
    /// session and the pane's alike.
    ///
    /// M5: the permission gate ([`GatedTools`]) owns prompting at the boundary;
    /// this inner seam is held open so a prompt-mode grant asks the user
    /// exactly once. It is a function rather than two literals because the pane
    /// needs the identical configuration and a second copy is a second chance
    /// for one of them to regress to `ScratchConfig::default()` and
    /// double-prompt with nobody noticing.
    fn chat_scratch(config: &Config) -> ScratchConfig {
        ScratchConfig {
            secret_env: config
                .tools
                .scratch
                .env
                .iter()
                .map(|(var, name)| (var.clone(), name.clone()))
                .collect(),
            ..ScratchConfig::always_confirmed()
        }
    }

    /// Build an executor over an already-open `Memory` (used by tests with a
    /// fixture memory, and by callers that own their own open). The caller
    /// keeps responsibility for closing that memory.
    pub fn from_memory(memory: Memory) -> Self {
        Self::over(Arc::new(memory), WriteLane::new())
    }

    /// [`MemoryTools::from_memory`] for a caller that already **shares** its
    /// handle — the pane, which holds one `Memory` and one lease for the length
    /// of the session and writes through it from more than one task.
    ///
    /// Two things `from_memory` cannot express and this can: the handle arrives
    /// as an `Arc` because the owner keeps its own reference, and the write lane
    /// arrives from outside because the owner's *other* writer has to take the
    /// same one. `owner` stays `None` either way — the caller closes, and the
    /// pane's close is ordered against putting the terminal back.
    ///
    /// Rejected: making `from_memory` take `Arc<Memory>` and a lane. It has a
    /// dozen call sites in tests whose whole point is a throwaway fixture
    /// memory with nothing to share it with, and every one would have grown two
    /// arguments that mean "no, and no".
    pub fn over(memory: Arc<Memory>, writes: WriteLane) -> Self {
        Self {
            mem: memory,
            worker: ToolRuntime::new(),
            scratch: ScratchConfig::default(),
            owner: None,
            vault: None,
            writes,
            diagnostics: Diagnostics::stderr(),
        }
    }

    /// Attach a shared vault handle (tests): enables per-run scratch secret
    /// injection. Egress redaction is a decorator ([`RedactingTools`]), not a
    /// `MemoryTools` concern.
    pub fn with_vault(mut self, vault: Option<SharedVault>) -> Self {
        self.vault = vault;
        self
    }

    /// Override the scratch permission/cap configuration (tests).
    pub fn with_scratch(mut self, scratch: ScratchConfig) -> Self {
        self.scratch = scratch;
        self
    }

    /// Override where execute-time diagnostics go. The pane installs a sink
    /// that does not print; the default is stderr, which is the CLI path.
    pub fn with_diagnostics(mut self, diagnostics: Diagnostics) -> Self {
        self.diagnostics = diagnostics;
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
        let writes = self.writes.clone();
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
                // The lane is entered INSIDE the bounded wait, not around it:
                // waiting for another writer is part of the 60s a tool call is
                // allowed to take, so a busy lane times out as a tool timeout
                // rather than stalling the caller past its own budget.
                rt.block_on(async move {
                    let _lane = writes.enter().await;
                    memory.derive(&concepts, &parent_of).await
                })
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
        // Resolve the configured secret injections now — after confirm,
        // before spawn. Any failure aborts before the script starts, so a
        // script never runs half-injected and no value reaches an error.
        let injected =
            match scratch::resolve_injection(&self.scratch.secret_env, self.vault.as_ref()) {
                Ok(env) => env,
                Err(message) => return message,
            };
        let timeout =
            Duration::from_secs(params.timeout_secs.unwrap_or(SCRATCH_DEFAULT_TIMEOUT_SECS));
        match scratch::run_script(
            &params.code,
            params.language,
            timeout,
            self.scratch.max_output_bytes,
            &injected,
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
        // The raw LamboError Display can carry store/connection material
        // (`Store` wraps backend messages naming DSN hosts), so neither the
        // terminal nor the model sees it — same discipline as `gate_panicked`.
        drop(error);
        let notice = text::get("tools.memory_tool_failed");
        self.diagnostics.emit(&format!("{what}: {notice}"));
        format!("{what}: {notice}")
    }

    fn lambo_run_err(&self, what: &str, run: ToolRunError) -> String {
        match run {
            ToolRunError::TimedOut => format!("{what}: {}", text::get("tools.tool_timeout")),
            ToolRunError::Panicked | ToolRunError::Unavailable => {
                self.diagnostics
                    .emit(&format!("{what}: tool runtime unavailable or panicked"));
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
                // A panic payload is arbitrary data — it may carry vault
                // values — so it is dropped, never formatted.
                drop(payload);
                self.diagnostics.emit(text::get("tools.tool_panicked"));
                tool_internal_error()
            }
        }
    }
}

/// The CLI-facing factory: open Memory for chat and compose the boundary —
/// **permission gate → egress redaction → tools** — and hand it to the chat
/// loop. Order is deliberate, one enforcement per concern:
///
/// 1. [`GatedTools`] decides *whether* the call may run at all (deny/prompt
///    answers never execute anything and so never surface a result to scan);
/// 2. the inner executor runs;
/// 3. [`RedactingTools`] scans the final result string against every vault
///    value post-execute, pre-history — before the model or the graph can see
///    it. Every current and future tool crosses that single boundary.
///
/// `vault` is the shared handle opened by the chat entry point. When it is
/// `None` (locked or missing home), chat still runs: a vault you cannot open
/// cannot leak values either, and one stderr notice says so — the loop is
/// never blocked on secret availability.
///
/// Lives here, not in `cli`, so `cli::chat` stays free of any
/// `crate::memory`/`provision` reference (M3 pins).
pub fn executor_for_chat(config: &Config, vault: Option<SharedVault>) -> Arc<dyn ToolExecutor> {
    if vault.is_none() {
        eprintln!("{}", text::get("tools.vault_unavailable"));
    }
    let grants = config.permissions.grants();
    let inner: Arc<dyn ToolExecutor> = match MemoryTools::for_chat(config, vault.clone()) {
        Some(tools) => tools,
        None => {
            eprintln!("{}", text::get("tools.chat_memory_unavailable"));
            Arc::new(NoopExecutor)
        }
    };
    let mcp = Arc::new(crate::mcp_host::McpTools::from_config(
        config,
        vault.clone(),
    ));
    let composite: Arc<dyn ToolExecutor> = Arc::new(CompositeTools::new(inner, mcp));
    compose_chat_stack(composite, vault, grants, None, Diagnostics::stderr())
}

/// A composed tool stack and the notices assembling it produced.
///
/// [`executor_for_chat`] writes its notices to stderr because the CLI owns the
/// terminal and stderr is where a notice belongs. Under ratatui's alternate
/// screen there is no such place: any write to stdout or stderr lands inside
/// the frame and corrupts it. So on the path a pane uses, a notice is a value
/// and the caller decides where it goes — into the view model, in the pane's
/// case.
///
/// Rejected: a callback the factory calls with each notice. It puts the
/// decision back at assembly time, which is exactly where it does not belong —
/// and it is harder to test than a `Vec` a caller can assert on.
pub struct ChatStack {
    pub tools: Arc<dyn ToolExecutor>,
    /// Rendered sentences from `en.toml`, in the order assembly produced them.
    /// Empty is the ordinary case.
    pub notices: Vec<String>,
}

/// [`executor_for_chat`] for a caller that has **already opened** `Memory` and
/// holds the single-writer lease itself.
///
/// This is the half of `executor_for_chat` that is not memory acquisition. The
/// pane takes the lease for the length of the session and documents itself as
/// an ordinary holder of it; `executor_for_chat` opens a `Memory` of its own,
/// and the two cannot both be true in one process — the second open is refused
/// with Mooshik's own conflict sentence. So the composition is separated from
/// the open rather than duplicated: both paths build **the same stack**, in the
/// same documented order, through [`compose_chat_stack`].
///
/// What the caller supplies that the CLI does not:
///
/// * `memory` and `writes` — the handle it already owns, and the lane its
///   *other* writer takes, so two tasks writing through one handle do not race
///   each other through lambo's optimistic replan (see [`WriteLane`]).
/// * `confirm` — because the default gate prompt reads **stdin**, and a gate
///   reading stdin while ratatui owns the terminal hangs the pane with no way
///   out. It is a required parameter, not an `Option`: a caller on this path
///   has to have decided, and a default that hangs is not a default.
///
/// There is no `Memory` fallback here. `executor_for_chat` degrades to
/// [`NoopExecutor`] when its open fails; a caller on this path has an open
/// handle in its hand, so the case does not exist.
pub fn executor_over_memory(
    config: &Config,
    vault: Option<SharedVault>,
    memory: Arc<Memory>,
    writes: WriteLane,
    confirm: Confirm,
    diagnostics: Diagnostics,
) -> ChatStack {
    let mut notices = Vec::new();
    if vault.is_none() {
        notices.push(text::get("tools.vault_unavailable").to_owned());
    }
    let grants = config.permissions.grants();
    let inner: Arc<dyn ToolExecutor> = Arc::new(
        MemoryTools::over(memory, writes)
            .with_vault(vault.clone())
            .with_scratch(MemoryTools::chat_scratch(config))
            .with_diagnostics(diagnostics.clone()),
    );
    let mcp = Arc::new(
        crate::mcp_host::McpTools::from_config(config, vault.clone())
            .with_diagnostics(diagnostics.clone()),
    );
    let composite: Arc<dyn ToolExecutor> = Arc::new(CompositeTools::new(inner, mcp));
    ChatStack {
        tools: compose_chat_stack(composite, vault, grants, Some(confirm), diagnostics),
        notices,
    }
}

/// The sibling factory to [`executor_for_chat`]: the [`RecallInjector`] the
/// chat `Session` installs, so turns dropped for context pressure come back as
/// recalled memory instead of vanishing.
///
/// `tools` is the stack [`executor_for_chat`] just built, shared by `Arc` and
/// **not** re-opened: `MemoryTools::for_chat` already owns the one open
/// `Memory`, and a second open would contend for lambo's single-writer session
/// lease. Sharing the stack also means an injected recall crosses the same
/// permission gate and the same egress redactor as a model-issued one.
///
/// Lives here, not in `cli` or `companion`, for the same reason
/// [`executor_for_chat`] does: those two must stay free of any
/// `crate::memory` reference (M3 pins).
pub fn recall_for_chat(config: &Config, tools: Arc<dyn ToolExecutor>) -> Arc<dyn RecallInjector> {
    Arc::new(recall::GraphRecall::new(tools, config))
}

/// The chat boundary composition, shared verbatim by [`executor_for_chat`]
/// and the behavioral composition pin in `tests`: egress redaction wraps the
/// inner executor (even the No-op fallback), the permission gate wraps
/// redaction. Extracted as a named seam so dropping [`RedactingTools`] — or
/// reordering gate/redaction — is caught by driving this function, not only
/// by reading its source.
/// `confirm` is how a prompted grant asks. `None` is the CLI's answer — the
/// interactive stdin prompt [`GatedTools`] installs for itself — and `Some` is
/// for a caller that owns the terminal and cannot let anything read stdin
/// behind its back. It is threaded through this one seam rather than given a
/// second composition function, because two compositions is how the gate ends
/// up in front of one of them and behind the other.
fn compose_chat_stack(
    inner: Arc<dyn ToolExecutor>,
    vault: Option<SharedVault>,
    grants: Grants,
    confirm: Option<Confirm>,
    diagnostics: Diagnostics,
) -> Arc<dyn ToolExecutor> {
    let redacting = Arc::new(RedactingTools::new(inner, vault));
    // Spelled out per arm rather than `Arc::new(gated.maybe_confirm(..))`, so
    // the wrap order is legible — and pinnable — as one expression either way.
    match confirm {
        Some(confirm) => Arc::new(
            GatedTools::new(redacting, grants)
                .with_confirm(confirm)
                .with_diagnostics(diagnostics),
        ),
        None => Arc::new(GatedTools::new(redacting, grants).with_diagnostics(diagnostics)),
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

#[cfg(test)]
mod tests;
