//! M5 — the permission gate: the ONE enforcement point at the tool-call
//! boundary (PLAN M5, SPEC *Autonomy is granted, not configured*).
//!
//! [`GatedTools`] wraps any [`ToolExecutor`] and is the only place a grant is
//! checked, because a check duplicated per tool is a check that will be
//! forgotten by the fourth tool:
//!
//! * [`ToolExecutor::specs`] filters out every ungranted tool, so a small
//!   model neither sees nor calls what was never granted.
//! * [`ToolExecutor::execute`] checks the resolved grant set **before** the
//!   inner executor runs; `allow` passes through, `prompt` defers to the user
//!   (M4's confirm seam moves here so it fires exactly once), and `deny`
//!   returns the contained refusal string — never a panic, never config paths
//!   or values in the model-visible message.
//!
//! Enforcement reads configuration only ([`Grants`]); the graph is never a
//! permission authority, however canonical a concept claims to be (pinned by a
//! test below).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use serde_json::Value;

use super::scratch::answer_yes;
use super::{panic_message, tool_internal_error, ToolExecutor, ToolSpec};
use crate::config::{GrantMode, Grants};
use crate::text;

/// Whether the user grants a prompted tool call. Receives the tool name.
pub type Confirm = Box<dyn Fn(&str) -> bool + Send + Sync>;

/// The interactive prompt, fail-closed like the M4 seam it replaces at this
/// boundary: anything but an explicit yes refuses.
fn interactive_confirm(tool: &str) -> bool {
    eprint!(
        "{}",
        text::get("permissions.prompt").replace("{tool}", tool)
    );
    use std::io::Write as _;
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    answer_yes(&line)
}

/// A [`ToolExecutor`] that enforces the resolved `[permissions]` grant set in
/// front of an inner executor. Chat composes: `executor_for_chat` → gate →
/// session; one choke point in front of every tool.
pub struct GatedTools {
    inner: Arc<dyn ToolExecutor>,
    grants: Grants,
    confirm: Confirm,
}

impl GatedTools {
    pub fn new(inner: Arc<dyn ToolExecutor>, grants: Grants) -> Self {
        Self {
            inner,
            grants,
            confirm: Box::new(interactive_confirm),
        }
    }

    /// Replace the prompt callback (tests).
    pub fn with_confirm(self, confirm: Confirm) -> Self {
        Self { confirm, ..self }
    }

    /// The resolved grant set this gate enforces.
    pub fn grants(&self) -> &Grants {
        &self.grants
    }

    fn authorized(&self, name: &str) -> bool {
        match self.grants.decision_for(name).mode {
            GrantMode::Allow => true,
            // Prompt fires only when the mode is prompt; allow executes
            // without asking, deny refuses without asking.
            GrantMode::Prompt => (self.confirm)(name),
            GrantMode::Deny => false,
        }
    }
}

impl ToolExecutor for GatedTools {
    fn specs(&self) -> Vec<ToolSpec> {
        self.inner
            .specs()
            .into_iter()
            .filter(|spec| self.grants.advertised(&spec.name))
            .collect()
    }

    fn execute(&self, name: &str, arguments: &Value) -> String {
        match catch_unwind(AssertUnwindSafe(|| self.authorized(name))) {
            Ok(true) => self.inner.execute(name, arguments),
            Ok(false) => text::get("permissions.denied").to_owned(),
            Err(payload) => {
                eprintln!("permission gate panicked: {}", panic_message(&payload));
                tool_internal_error()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    const RECALL: &str = "lambo_recall";
    const DERIVE: &str = "lambo_derive";
    const SCRATCH: &str = "run_scratch_script";

    /// An inner executor that records every call it actually receives.
    struct Recorder {
        specs: Vec<ToolSpec>,
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                specs: super::super::tool_specs(),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn with_spec(mut self, name: &str) -> Self {
            self.specs.push(ToolSpec {
                name: name.to_owned(),
                description: "future mcp tool".into(),
                parameters: serde_json::json!({ "type": "object", "properties": {} }),
            });
            self
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ToolExecutor for Recorder {
        fn specs(&self) -> Vec<ToolSpec> {
            self.specs.clone()
        }

        fn execute(&self, name: &str, _arguments: &Value) -> String {
            self.calls.lock().unwrap().push(name.to_owned());
            format!("ran {name}")
        }
    }

    fn grants_from(table: &str) -> Grants {
        Config::from_toml_and_env(&format!("[permissions]\n{table}\n"), [])
            .unwrap()
            .permissions
            .grants()
    }

    fn gated(inner: Arc<dyn ToolExecutor>, table: &str) -> GatedTools {
        GatedTools::new(inner, grants_from(table)).with_confirm(Box::new(|_| false))
    }

    #[test]
    fn ungranted_tools_are_not_advertised() {
        let recorder = Arc::new(Recorder::new());
        let gate = gated(recorder, "memory = ['recall', 'derive']");
        let names: Vec<String> = gate.specs().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec![RECALL, DERIVE, SCRATCH]);
    }

    #[test]
    fn denied_execute_is_contained_and_never_reaches_the_inner_executor() {
        let recorder = Arc::new(Recorder::new());
        let gate = gated(recorder.clone(), "scratch = 'deny'");
        let out = gate.execute(
            SCRATCH,
            &serde_json::json!({ "language": "bash", "code": "echo hi" }),
        );
        assert_eq!(out, text::get("permissions.denied"));
        assert!(
            recorder.calls().is_empty(),
            "a denied call must not dispatch"
        );
    }

    #[test]
    fn prompt_fires_only_in_prompt_mode() {
        // allow: no prompt.
        let prompts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = prompts.clone();
        let recorder = Arc::new(Recorder::new());
        let gate = GatedTools::new(recorder.clone(), grants_from("scratch = 'allow'"))
            .with_confirm(Box::new(move |_| {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                false
            }));
        assert_eq!(
            gate.execute(
                SCRATCH,
                &serde_json::json!({ "language": "bash", "code": "echo hi" })
            ),
            "ran run_scratch_script"
        );
        assert_eq!(prompts.load(std::sync::atomic::Ordering::SeqCst), 0);

        // deny: no prompt either.
        let counter = prompts.clone();
        let gate = GatedTools::new(recorder.clone(), grants_from("scratch = 'deny'")).with_confirm(
            Box::new(move |_| {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                true
            }),
        );
        assert_eq!(
            gate.execute(
                SCRATCH,
                &serde_json::json!({ "language": "bash", "code": "echo hi" })
            ),
            text::get("permissions.denied")
        );
        assert_eq!(prompts.load(std::sync::atomic::Ordering::SeqCst), 0);

        // prompt: asked once; a yes executes, a no refuses contained.
        let counter = prompts.clone();
        let gate =
            GatedTools::new(recorder.clone(), grants_from("")).with_confirm(Box::new(move |_| {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                true
            }));
        assert_eq!(
            gate.execute(
                SCRATCH,
                &serde_json::json!({ "language": "bash", "code": "echo hi" })
            ),
            "ran run_scratch_script"
        );
        assert_eq!(prompts.load(std::sync::atomic::Ordering::SeqCst), 1);

        let gate = gated(recorder, "");
        assert_eq!(
            gate.execute(
                SCRATCH,
                &serde_json::json!({ "language": "bash", "code": "echo hi" })
            ),
            text::get("permissions.denied")
        );
    }

    #[test]
    fn future_mcp_tools_pass_through_the_same_gate() {
        let recorder = Arc::new(Recorder::new().with_spec("mcp.github.create_issue"));
        let open = gated(recorder.clone(), "'mcp.github.*' = 'allow'");
        let names: Vec<String> = open.specs().into_iter().map(|s| s.name).collect();
        assert!(
            names.iter().any(|n| n == "mcp.github.create_issue"),
            "{names:?}"
        );
        assert_eq!(
            open.execute(
                "mcp.github.create_issue",
                &serde_json::json!({ "title": "x" })
            ),
            "ran mcp.github.create_issue"
        );

        let shut = gated(
            Arc::new(Recorder::new().with_spec("mcp.github.create_issue")),
            "",
        );
        let names: Vec<String> = shut.specs().into_iter().map(|s| s.name).collect();
        assert!(
            !names.iter().any(|n| n == "mcp.github.create_issue"),
            "{names:?}"
        );
        assert_eq!(
            shut.execute("mcp.github.create_issue", &serde_json::json!({})),
            text::get("permissions.denied")
        );
    }

    #[test]
    fn the_gate_never_consults_the_graph() {
        // Same pin technique as the M3/M4 seams: enforcement reads config
        // only. The graph is never a permission authority.
        let production = include_str!("permissions.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !production.contains("crate::memory"),
            "the gate must never reference the graph module"
        );
        assert!(
            !production.contains("Memory"),
            "the gate must never touch a graph handle"
        );
    }
}
