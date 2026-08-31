//! M10 — the MCP host: spawns configured MCP servers and exposes their tools as
//! `mcp.<server>.<tool>` companion tools.
//!
//! # Design decisions
//!
//! **Spawn-if-expose.** A server with an empty `expose` allowlist is never
//! spawned — it is functionally inert. The `[mcp_servers]` table is data,
//! held inert until an operator explicitly names what to expose.
//!
//! **Lazy spawn.** Servers spawn on first use (a `specs()` or `execute()` call),
//! not at chat startup, so a companion that never calls MCP tools pays no
//! process-lifecycle overhead and startup remains bounded.
//!
//! **Reconnect-on-crash, bounded.** If a child is gone when a tool is called, a
//! single restart attempt is made — one spawn, one retry of the RPC. If the
//! restart also fails, the call returns a contained error string and the server's
//! live slot stays closed (next call may retry again). The companion never sees
//! a dead process or a raw panic.
//!
//! **Vault-ref resolution at spawn.** The `env` map on each config entry names
//! vault secrets — the operator writes *names*, never values. At spawn time
//! each name is resolved through the vault handle. Missing secrets or an
//! unavailable vault cause that server to fail closed (contained error, no
//! tools contributed). Other servers are unaffected.
//!
//! **Panic containment.** Every `execute` dispatch is wrapped in
//! [`std::panic::catch_unwind`] — a panicking MCP client task never kills the
//! process or the chat loop.
//!
//! **Tool naming.** `mcp.<server>.<tool>` — the `<server>` is the config-key
//! verbatim, `<tool>` is the server-reported tool name. The M5 permission-gate
//! prefix grammar (`mcp.github.*`) matches these names with the least surprising
//! mapping.
//!
//! **Composition.** `McpTools` sits inside the same `GatedTools(RedactingTools(.))`
//! chain as the memory tools, via a small composite in `src/tools/mod.rs`.
//! Egress redaction scans MCP results too (it scans every inner result), and the
//! gate filters `mcp.*` tools according to the grant table.
//!
//! **All async rmcp work runs on the ToolRuntime worker.** The `execute` method
//! is synchronous and called from the chat loop's Tokio runtime; it submits a
//! closure to the worker, which runs `rt.block_on` on its own dedicated thread.
//! This avoids Tokio's no-block_on-from-within-a-runtime restriction.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::time::timeout;

use crate::companion::{ToolExecutor, ToolSpec};
use crate::config::Config;
use crate::text;
use crate::tools::{Diagnostics, ToolRunError, ToolRuntime};
use crate::vault::{lock_shared, SharedVault};

/// Bound on spawning one MCP server (child startup + MCP initialize + discovery).
const MCP_SPAWN_WAIT: Duration = Duration::from_secs(30);
/// Bound on one MCP tool call round-trip.
const MCP_CALL_WAIT: Duration = Duration::from_secs(60);

/// Concrete rmcp client session type.
type Session = rmcp::service::RunningService<rmcp::service::RoleClient, ()>;

/// A live MCP session and the tools it reported.
struct LiveServer {
    session: Session,
    specs: Vec<ToolSpec>,
}

/// Why a `call_tool` did not produce a successful text result.
enum CallError {
    /// The tool ran and returned `isError: true`. Its text content is the
    /// model-visible contained error — the server's message, surfaced but
    /// contained.
    ToolError(String),
    /// The session or transport failed (dead child, closed stream, protocol
    /// error). The detail is withheld from the model.
    Transport,
}

impl LiveServer {
    fn alive(&self) -> bool {
        !self.session.is_closed()
    }

    /// Call a tool through the live rmcp session. Runs on the worker runtime
    /// (the session's background tasks live on that runtime).
    async fn call_tool(&self, tool: &str, arguments: &Value) -> Result<String, CallError> {
        if self.session.is_closed() {
            return Err(CallError::Transport);
        }
        let map = arguments.as_object().cloned().unwrap_or_default();
        let params = rmcp::model::CallToolRequestParams::new(tool.to_owned()).with_arguments(map);
        let result = self
            .session
            .call_tool(params)
            .await
            .map_err(|_| CallError::Transport)?;
        // Concatenate text content blocks (the brief's "text of content blocks").
        let parts: Vec<&str> = result
            .content
            .iter()
            .filter_map(|block| match block {
                rmcp::model::ContentBlock::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect();
        if result.is_error.unwrap_or(false) {
            let msg = parts.join("\n");
            return Err(CallError::ToolError(if msg.is_empty() {
                "tool returned isError with no text content".into()
            } else {
                msg
            }));
        }
        Ok(parts.join("\n"))
    }
}

/// Per-server config captured from the `[mcp_servers.<name>]` table.
#[derive(Clone)]
struct ServerConfig {
    name: String,
    command: String,
    args: Vec<String>,
    secret_env: Vec<(String, String)>,
    expose: HashSet<String>,
}

/// A [`ToolExecutor`] that wraps zero or more configured MCP servers as
/// `mcp.<server>.<tool>` companion tools.
pub struct McpTools {
    worker: ToolRuntime,
    servers: Vec<Arc<ServerConfig>>,
    lives: Vec<Arc<Mutex<Option<LiveServer>>>>,
    vault: Option<SharedVault>,
    /// Hard bound per MCP tool call; the worker-release firebreak for a hung
    /// but alive child (P2-M10-1). Defaults to [`MCP_CALL_WAIT`]; tests shrink
    /// it to keep the hung-child pin fast.
    call_wait: Duration,
    /// Have we attempted the initial spawn of all servers?
    spawned: Mutex<bool>,
    /// Cached merged tool specs, refreshed after spawn.
    all_specs: Mutex<Vec<ToolSpec>>,
    /// Execute-time diagnostics. Stderr on the CLI path; a channel on the pane.
    diagnostics: Diagnostics,
}

impl McpTools {
    /// Build an MCP tool executor from the resolved config.
    ///
    /// Servers whose `expose` list is empty are discarded at construction (they
    /// are never spawned). Servers with a non-empty list are spawned lazily on
    /// first use.
    pub fn from_config(config: &Config, vault: Option<SharedVault>) -> Self {
        let mut servers: Vec<ServerConfig> = Vec::new();
        for (name, cfg) in &config.mcp_servers {
            if cfg.expose.is_empty() {
                continue; // inert: never spawned
            }
            servers.push(ServerConfig {
                name: name.clone(),
                command: cfg.command.clone(),
                args: cfg.args.clone(),
                secret_env: cfg
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                expose: cfg.expose.iter().cloned().collect(),
            });
        }
        let n = servers.len();
        McpTools {
            worker: ToolRuntime::new(),
            servers: servers.into_iter().map(Arc::new).collect(),
            lives: (0..n).map(|_| Arc::new(Mutex::new(None))).collect(),
            vault,
            call_wait: MCP_CALL_WAIT,
            spawned: Mutex::new(false),
            all_specs: Mutex::new(Vec::new()),
            diagnostics: Diagnostics::stderr(),
        }
    }

    /// Override where execute-time diagnostics go. The pane installs a sink
    /// that does not print; the default is stderr.
    pub fn with_diagnostics(mut self, diagnostics: Diagnostics) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    /// Override the per-call bound (tests only; shrinks the hung-child pin).
    #[cfg(test)]
    fn with_call_wait(mut self, wait: Duration) -> Self {
        self.call_wait = wait;
        self
    }

    /// Ensure all servers are spawned and tool specs computed.
    /// Must be called from the sync (chat) thread.
    fn ensure_spawned(&self) {
        let mut spawned = self.spawned.lock();
        if *spawned {
            return;
        }
        // Mark attempted BEFORE spawning; on failure leave it false so the
        // next specs()/execute retries rather than caching a dead slot
        // forever (P3-M10-2). The per-execute phase-1 respawn still covers
        // tool calls; this covers the spec list recovering on a later call.
        if self.attempt_spawn() {
            *spawned = true;
        }
    }

    /// One spawn pass over all servers; true when every non-inert server has
    /// a live slot. Runs on the worker runtime. Caller holds `spawned`.
    fn attempt_spawn(&self) -> bool {
        let servers = self.servers.clone();
        let lives = self.lives.clone();
        let vault = self.vault.clone();
        let diagnostics = self.diagnostics.clone();
        let total_budget = MCP_SPAWN_WAIT * self.servers.len().max(1) as u32;

        let ok = self
            .worker
            .run(
                move |rt: &Runtime| {
                    rt.block_on(spawn_all(&servers, &lives, vault.as_ref(), &diagnostics))
                },
                total_budget,
            )
            .is_ok();
        self.refresh_specs();
        ok && self.lives.iter().all(|slot| slot.lock().is_some())
    }

    /// Rebuild the merged spec list from whatever lives slots hold.
    fn refresh_specs(&self) {
        let mut all = Vec::new();
        for slot in &self.lives {
            let guard = slot.lock();
            if let Some(live) = guard.as_ref() {
                all.extend(live.specs.clone());
            }
        }
        *self.all_specs.lock() = all;
    }

    /// The synchronous execute path; all rmcp interaction is offloaded to the worker.
    fn dispatch(&self, full_name: &str, arguments: &Value) -> String {
        // Parse `mcp.<server>.<tool>`.
        let (idx, tool_name) = match parse_mcp_name(full_name, &self.servers) {
            Ok(v) => v,
            Err(e) => return e,
        };

        // Verify expose membership.
        if !self.servers[idx].expose.contains(tool_name) {
            return text::get("tools.mcp_tool_unknown")
                .replace("{server}", &self.servers[idx].name)
                .replace("{tool}", tool_name);
        }

        self.ensure_spawned();

        let slot = self.lives[idx].clone();
        let cfg = self.servers[idx].clone();
        let vault = self.vault.clone();
        let tool = tool_name.to_owned();
        let args = arguments.clone();
        let diagnostics = self.diagnostics.clone();

        let call_wait = self.call_wait;
        let outcome: Result<Result<String, String>, ToolRunError> = self.worker.run(
            move |rt: &Runtime| {
                let wait = call_wait;
                rt.block_on(execute_on_worker(
                    &slot,
                    &cfg,
                    vault.as_ref(),
                    &tool,
                    &args,
                    wait,
                    &diagnostics,
                ))
            },
            call_wait + MCP_SPAWN_WAIT,
        );

        match outcome {
            Ok(Ok(output)) => output,
            Ok(Err(msg)) => {
                self.diagnostics
                    .emit(&format!("mcp.{}: {msg}", self.servers[idx].name));
                text::get("tools.mcp_tool_failed")
                    .replace("{server}", &self.servers[idx].name)
                    .replace("{tool}", tool_name)
            }
            Err(ToolRunError::TimedOut) => {
                let msg = text::get("tools.tool_timeout");
                self.diagnostics
                    .emit(&format!("mcp.{}: {msg}", self.servers[idx].name));
                text::get("tools.internal_error").to_owned()
            }
            Err(_) => {
                self.diagnostics
                    .emit(&format!("mcp.{}: worker panicked", self.servers[idx].name));
                text::get("tools.internal_error").to_owned()
            }
        }
    }
}

impl ToolExecutor for McpTools {
    fn specs(&self) -> Vec<ToolSpec> {
        self.ensure_spawned();
        self.all_specs.lock().clone()
    }

    fn execute(&self, full_name: &str, arguments: &Value) -> String {
        match catch_unwind(AssertUnwindSafe(|| self.dispatch(full_name, arguments))) {
            Ok(output) => output,
            Err(payload) => {
                drop(payload);
                self.diagnostics.emit(text::get("tools.tool_panicked"));
                text::get("tools.internal_error").to_owned()
            }
        }
    }
}

// ---- async helpers (called on the worker runtime via rt.block_on) ----------

/// Parse `mcp.<server>.<tool>` into (server_index, tool_name).
fn parse_mcp_name<'a>(
    full_name: &'a str,
    servers: &[Arc<ServerConfig>],
) -> Result<(usize, &'a str), String> {
    let stripped = full_name
        .strip_prefix("mcp.")
        .ok_or_else(|| format!("tool '{full_name}' must start with 'mcp.'"))?;
    let dot = stripped
        .find('.')
        .ok_or_else(|| format!("tool '{full_name}': expected 'mcp.<server>.<tool>'"))?;
    let server_name = &stripped[..dot];
    let tool_name = &stripped[dot + 1..];
    if server_name.is_empty() || tool_name.is_empty() {
        return Err(format!(
            "tool '{full_name}': server and tool must not be empty"
        ));
    }
    let idx = servers
        .iter()
        .position(|s| s.name == server_name)
        .ok_or_else(|| format!("unknown MCP server '{server_name}'"))?;
    Ok((idx, tool_name))
}

/// Spawn all servers that have not yet been spawned (lazy init).
async fn spawn_all(
    servers: &[Arc<ServerConfig>],
    lives: &[Arc<Mutex<Option<LiveServer>>>],
    vault: Option<&SharedVault>,
    diagnostics: &Diagnostics,
) {
    for (idx, cfg) in servers.iter().enumerate() {
        let result = timeout(MCP_SPAWN_WAIT, spawn_one(cfg, vault, diagnostics)).await;
        let live = match result {
            Ok(Some(live)) => live,
            _ => continue,
        };
        lives[idx].lock().replace(live);
    }
}

/// Drain one child's stderr into the diagnostics sink for the life of the
/// process. A piped stderr that nobody reads fills the pipe buffer and blocks
/// the child mid-write, which would turn a noisy server into a hung one. Each
/// complete line becomes one diagnostic; the task ends when the pipe closes,
fn drain_child_stderr(stderr: tokio::process::ChildStderr, diagnostics: Diagnostics, name: &str) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let name = name.to_owned();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => break, // EOF: the child exited
                Err(_) => break,
            };
            diagnostics.emit(&format!("mcp_host: '{name}' stderr: {}", line.trim_end()));
        }
    });
}

/// Spawn one MCP server: child process, rmcp handshake, tool discovery.
async fn spawn_one(
    cfg: &ServerConfig,
    vault: Option<&SharedVault>,
    diagnostics: &Diagnostics,
) -> Option<LiveServer> {
    // Resolve vault refs.
    let env = match resolve_env(&cfg.secret_env, vault) {
        Ok(env) => env,
        Err(msg) => {
            diagnostics.emit(&msg);
            return None;
        }
    };

    // Build child command.
    let mut cmd = tokio::process::Command::new(&cfg.command);
    cmd.args(&cfg.args);
    for (k, v) in &env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // rmcp's builder re-applies its own stdio defaults, so stderr must be
    // piped here rather than on the Command. A piped stderr that nobody drains
    // fills the pipe buffer and blocks the child mid-write, which would turn a
    // noisy server into a hung one; the drain below runs for the life of the
    // process and ends when the pipe closes (the child exited).
    let (transport, stderr) = match rmcp::transport::child_process::TokioChildProcess::builder(cmd)
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(pair) => pair,
        Err(e) => {
            diagnostics.emit(&format!(
                "mcp_host: failed to spawn '{}' ({}): {e}",
                cfg.name, cfg.command
            ));
            return None;
        }
    };
    if let Some(stderr) = stderr {
        drain_child_stderr(stderr, diagnostics.clone(), &cfg.name);
    }

    // Handshake and get client session.
    let session = match rmcp::service::serve_client((), transport).await {
        Ok(s) => s,
        Err(e) => {
            diagnostics.emit(&format!(
                "mcp_host: MCP handshake failed for '{}' ({}): {e}",
                cfg.name, cfg.command
            ));
            return None;
        }
    };

    // Discover tools.
    let tools = match session.peer().list_all_tools().await {
        Ok(t) => t,
        Err(e) => {
            diagnostics.emit(&format!(
                "mcp_host: tools/list failed for '{}' ({}): {e}",
                cfg.name, cfg.command
            ));
            return None;
        }
    };

    // Build ToolSpecs filtered by the expose allowlist.
    let prefix = format!("mcp.{}.", cfg.name);
    let mut specs = Vec::new();
    for tool in &tools {
        let short = tool.name.as_ref();
        if !cfg.expose.contains(short) {
            continue;
        }
        let full_name = format!("{prefix}{short}");
        let params: Value = tool.input_schema.as_ref().clone().into();
        specs.push(ToolSpec {
            name: full_name,
            description: tool.description.as_deref().unwrap_or("").to_owned(),
            parameters: params,
        });
    }

    Some(LiveServer { session, specs })
}

/// Run one MCP tool call under a hard per-call wall-clock bound. A child that
/// is alive but never answers would otherwise pin the shared worker thread
/// indefinitely (P2-M10-1); the outer `ToolRuntime::run` budget fires on the
/// caller side and cannot free the worker. This timeout does.
async fn bounded_call(
    live: &LiveServer,
    tool: &str,
    arguments: &Value,
    wait: Duration,
) -> Result<String, CallError> {
    timeout(wait, live.call_tool(tool, arguments))
        .await
        .unwrap_or(Err(CallError::Transport))
}

/// Run one MCP tool call: ensures a live session (respawning once on the
/// first transport-level failure), calls the tool under a hard per-call bound,
/// and returns a fully-formed contained string. The slot lock is never held
/// across an await — each phase locks, takes/sets, and releases.
async fn execute_on_worker(
    slot: &Arc<Mutex<Option<LiveServer>>>,
    cfg: &ServerConfig,
    vault: Option<&SharedVault>,
    tool: &str,
    arguments: &Value,
    call_wait: Duration,
    diagnostics: &Diagnostics,
) -> Result<String, String> {
    // Phase 1: ensure a live session exists. Lock, decide, release.
    let needs_spawn = slot.lock().as_ref().is_none_or(|live| !live.alive());
    if needs_spawn {
        let fresh = timeout(MCP_SPAWN_WAIT, spawn_one(cfg, vault, diagnostics))
            .await
            .unwrap_or(None);
        *slot.lock() = fresh;
    }

    // Phase 2: take the live server out so no lock crosses the call await.
    let live = slot
        .lock()
        .take()
        .ok_or_else(|| text::get("tools.mcp_spawn_failed").replace("{server}", &cfg.name))?;
    let result = bounded_call(&live, tool, arguments, call_wait).await;

    // Phase 3: carry through the result. On a transport failure, the session
    // is dead — spawn one replacement (bounded) and retry once.
    match result {
        Ok(text) => {
            *slot.lock() = Some(live);
            Ok(text)
        }
        // Tool-level error: the server's message is the model-visible
        // contained error; the session is still alive, put it back.
        Err(CallError::ToolError(msg)) => {
            *slot.lock() = Some(live);
            Ok(msg)
        }
        Err(CallError::Transport) => {
            drop(live);
            let revived = timeout(MCP_SPAWN_WAIT, spawn_one(cfg, vault, diagnostics))
                .await
                .unwrap_or(None);
            match revived {
                Some(revived) => {
                    let result = bounded_call(&revived, tool, arguments, call_wait).await;
                    match result {
                        Ok(text) => {
                            *slot.lock() = Some(revived);
                            Ok(text)
                        }
                        Err(CallError::ToolError(msg)) => {
                            *slot.lock() = Some(revived);
                            Ok(msg)
                        }
                        Err(CallError::Transport) => {
                            *slot.lock() = Some(revived);
                            Err(text::get("tools.mcp_tool_failed")
                                .replace("{server}", &cfg.name)
                                .replace("{tool}", tool))
                        }
                    }
                }
                None => Err(text::get("tools.mcp_spawn_failed").replace("{server}", &cfg.name)),
            }
        }
    }
}

// ---- vault resolution ------------------------------------------------------

/// Resolve vault secret names into environment variables.
fn resolve_env(
    secret_env: &[(String, String)],
    vault: Option<&SharedVault>,
) -> Result<HashMap<String, String>, String> {
    if secret_env.is_empty() {
        return Ok(HashMap::new());
    }
    let v = vault.ok_or_else(|| text::get("tools.mcp_servers_unavailable").to_owned())?;
    let guard = lock_shared(v);
    let mut env = HashMap::with_capacity(secret_env.len());
    for (var, secret_name) in secret_env {
        let token = guard
            .get(secret_name)
            .map_err(|_| text::get("tools.mcp_secret_missing").replace("{name}", secret_name))?;
        env.insert(var.clone(), token.expose().to_owned());
    }
    Ok(env)
}

#[cfg(test)]
mod tests;
