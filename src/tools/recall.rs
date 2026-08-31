//! The production [`RecallInjector`]: turns that leave the context window come
//! back as *recalled memory*, not as a summary.
//!
//! This is the seam the product thesis rests on — **the model does not
//! remember, the graph does** (PLAN, "Context pressure becomes the demo, not a
//! bug"). [`super::executor_for_chat`] builds the tool stack; this module's
//! sibling factory [`super::recall_for_chat`] builds the injector *over that
//! same stack*, so:
//!
//! * there is exactly one open [`lambo::Memory`] per chat (a second open would
//!   contend for the single-writer session lease);
//! * an injected recall crosses the *same* M5 permission gate and the *same*
//!   M6 egress redactor every model-issued `lambo_recall` crosses.
//!
//! **Nothing but a well-formed recall result is ever injected.** The tool seam
//! answers failures as plain *strings* (a permission refusal, a bounded-wait
//! timeout, the contained internal-error notice). The injected message is
//! prompt content, so each of those is worse than nothing: [`render`] parses
//! the result as a [`RecallResult`] and injects only on success. Every error
//! string fails that parse and yields `None`.
//!
//! **The recall is deliberately small.** It is injected into a window that is
//! *already* under pressure, so the query, the `top_k`, the lambo token budget
//! and the final rendered block are all capped here. `pack_messages` re-checks
//! the budget after injection and drops an oversized injection rather than
//! evicting the current turn, but a recall that is routinely dropped is a
//! recall that never happens — the caps below are what make it land.

use std::sync::Arc;

use lambo::RecallResult;
use serde_json::json;

use super::TOOL_RECALL;
use crate::companion::{Message, RecallInjector, Role, ToolExecutor};
use crate::config::{Config, GrantMode};
use crate::text;

/// Hits to pull for an injected recall. Smaller than the operator-facing
/// `mooshik recall` page (5): this competes with the live conversation.
const INJECT_TOP_K: usize = 3;
/// Lambo's own token budget for the rendered context block.
const INJECT_MAX_TOKENS: usize = 200;
/// No graph expansion: depth costs budget the live turn needs.
const INJECT_TRAVERSAL_DEPTH: usize = 0;
/// Cap on the assembled query text.
const MAX_QUERY_CHARS: usize = 512;
/// How many dropped user turns may add context to the query.
const MAX_DROPPED_TURNS: usize = 2;
/// Cap on each dropped turn's contribution, so one long turn cannot drown the
/// current one — which is the strongest signal.
const MAX_DROPPED_CHARS: usize = 160;
/// Hard cap on the whole injected message, header included (characters).
/// ~250 estimated tokens, comfortably inside a window under pressure.
const MAX_INJECTION_CHARS: usize = 1_000;

/// A [`RecallInjector`] backed by the chat session's own tool stack.
pub struct GraphRecall {
    /// The composed chat stack (gate → redaction → tools). Shared with the
    /// `Session`'s executor: same `Arc`, same open `Memory`, one lease.
    tools: Arc<dyn ToolExecutor>,
    agent_id: String,
    /// Whether `[permissions]` grants recall outright.
    ///
    /// Only [`GrantMode::Allow`] injects. `prompt` deliberately does not: an
    /// injection is automatic, and prompting for it would stop mid-packing to
    /// read stdin *while the chat loop is reading stdin*. A prompt-mode grant
    /// still lets the model call `lambo_recall` itself, which is where the
    /// question belongs.
    granted: bool,
}

impl GraphRecall {
    pub fn new(tools: Arc<dyn ToolExecutor>, config: &Config) -> Self {
        Self {
            tools,
            agent_id: config.session.agent.clone(),
            granted: matches!(
                config.permissions.grants().decision_for(TOOL_RECALL).mode,
                GrantMode::Allow
            ),
        }
    }
}

impl RecallInjector for GraphRecall {
    fn inject(&self, dropped: &[Message], current_user: &str) -> Option<Message> {
        if !self.granted {
            return None;
        }
        let query = build_query(dropped, current_user);
        if query.is_empty() {
            return None;
        }
        // `execute` is synchronous and already bounded by the tool worker's
        // per-call wait — the same bound every model-issued tool call runs
        // under. No async plumbing crosses this seam.
        let raw = self.tools.execute(
            TOOL_RECALL,
            &json!({
                "agent_id": self.agent_id,
                "query": query,
                "top_k": INJECT_TOP_K,
                "max_tokens": INJECT_MAX_TOKENS,
                "traversal_depth": INJECT_TRAVERSAL_DEPTH,
            }),
        );
        render(&raw)
    }
}

/// The current turn first (strongest signal), then a bounded tail of the
/// dropped *user* turns as context. Assistant and tool turns are left out:
/// they are the model's own words, and re-querying memory with them mostly
/// retrieves what the model already said.
fn build_query(dropped: &[Message], current_user: &str) -> String {
    let mut query = current_user.trim().to_owned();
    let mut used = 0;
    for message in dropped.iter().rev() {
        if used == MAX_DROPPED_TURNS || query.chars().count() >= MAX_QUERY_CHARS {
            break;
        }
        if message.role != Role::User {
            continue;
        }
        let extra = truncate_chars(message.content.trim(), MAX_DROPPED_CHARS);
        if extra.is_empty() {
            continue;
        }
        if !query.is_empty() {
            query.push(' ');
        }
        query.push_str(&extra);
        used += 1;
    }
    truncate_chars(query.trim(), MAX_QUERY_CHARS)
}

/// Build the injected message, or `None` when nothing useful came back.
///
/// The parse is the guard rail: a permission refusal, a timeout notice, the
/// contained internal-error string and any other tool answer are not valid
/// [`RecallResult`] JSON, so none of them can reach the model as context.
fn render(raw: &str) -> Option<Message> {
    let recalled: RecallResult = serde_json::from_str(raw).ok()?;
    if recalled.hits.is_empty() {
        return None;
    }
    // Lambo renders a context block "ready to hand to a model" (canonical
    // markers, blast-radius warnings); fall back to the hit texts if the
    // block came back empty. `warnings` are operational and stay out.
    let body = if recalled.context.trim().is_empty() {
        recalled
            .hits
            .iter()
            .map(|hit| hit.content.trim())
            .filter(|content| !content.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        recalled.context.trim().to_owned()
    };
    // The cap covers the WHOLE injected message, header included: what packing
    // has to fit is the message, not the part of it memory contributed.
    let header = text::get("companion.recall_injection");
    let body = clamp_block(
        &body,
        MAX_INJECTION_CHARS.saturating_sub(header.chars().count() + 1),
    );
    if body.is_empty() {
        return None;
    }
    Some(Message::system(format!("{header}\n{body}")))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect::<String>()
}

/// Truncate a rendered block on a line boundary where one exists, so a hit
/// never lands half-written in the model's context.
fn clamp_block(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.trim().to_owned();
    }
    let cut = truncate_chars(text, max_chars);
    match cut.rfind('\n') {
        Some(at) if at > 0 => cut[..at].trim().to_owned(),
        _ => cut.trim().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::ToolSpec;
    use crate::tools::MemoryTools;
    use serde_json::Value;
    use std::sync::Mutex;

    /// An inner executor that records every call and answers a fixed string.
    struct Canned {
        answer: String,
        calls: Mutex<Vec<String>>,
    }

    impl Canned {
        fn new(answer: impl Into<String>) -> Arc<Self> {
            Arc::new(Self {
                answer: answer.into(),
                calls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ToolExecutor for Canned {
        fn specs(&self) -> Vec<ToolSpec> {
            Vec::new()
        }

        fn execute(&self, name: &str, arguments: &Value) -> String {
            self.calls.lock().unwrap().push(format!(
                "{name}:{}",
                arguments["query"].as_str().unwrap_or_default()
            ));
            self.answer.clone()
        }
    }

    /// A recall answer built from lambo's own types and serialized exactly as
    /// `MemoryTools::run_recall` does, so the fixture cannot drift from the
    /// wire shape the injector actually parses.
    fn result_json(contents: &[&str], context: &str) -> String {
        let hits = contents
            .iter()
            .map(|content| lambo::RecallHit {
                node_id: lambo::NodeId::new(),
                content: (*content).to_owned(),
                concept_type: Some(lambo::ConceptType::Entity),
                score: 0.9,
                is_canonical: false,
                blast_radius: None,
            })
            .collect();
        serde_json::to_string(&RecallResult {
            hits,
            context: context.to_owned(),
            warnings: Vec::new(),
        })
        .unwrap()
    }

    fn config_with(table: &str) -> Config {
        Config::from_toml_and_env(&format!("[permissions]\n{table}\n"), []).unwrap()
    }

    fn dropped_turns() -> Vec<Message> {
        vec![
            Message::user("we agreed the deploy window is friday"),
            Message::assistant("noted", Vec::new()),
        ]
    }

    #[test]
    fn a_recall_result_is_rendered_into_one_system_message() {
        let inner = Canned::new(result_json(&["deploy window is friday"], ""));
        let recall = GraphRecall::new(inner.clone(), &Config::default());
        let injected = recall
            .inject(&dropped_turns(), "when do we ship?")
            .expect("a hit must be injected");
        assert_eq!(injected.role, Role::System);
        assert!(
            injected.content.contains("deploy window is friday"),
            "{}",
            injected.content
        );
        assert!(
            injected
                .content
                .contains(text::get("companion.recall_injection")),
            "{}",
            injected.content
        );
        // The current turn leads the query; the dropped user turn follows it,
        // and the assistant turn never joins it.
        let calls = inner.calls();
        assert_eq!(calls.len(), 1);
        let query = calls[0].strip_prefix("lambo_recall:").unwrap();
        assert!(query.starts_with("when do we ship?"), "{query}");
        assert!(query.contains("deploy window is friday"), "{query}");
        assert!(!query.contains("noted"), "{query}");
    }

    #[test]
    fn a_denied_grant_injects_nothing_and_never_calls_the_tool() {
        // Constraint 3: `[permissions]` may deny recall. Denied means the
        // tool is not called at all — and, critically, the refusal string is
        // never what lands in the model's context.
        let inner = Canned::new(result_json(&["should never be injected"], ""));
        let recall = GraphRecall::new(inner.clone(), &config_with("memory = 'deny'"));
        assert!(recall
            .inject(&dropped_turns(), "when do we ship?")
            .is_none());
        assert!(
            inner.calls().is_empty(),
            "a denied recall must not dispatch"
        );
    }

    #[test]
    fn a_prompt_grant_injects_nothing_and_never_prompts() {
        // An injection is automatic. Prompting for it would read stdin from
        // inside `pack_messages`, while the chat loop is reading stdin.
        let inner = Canned::new(result_json(&["should never be injected"], ""));
        let recall = GraphRecall::new(inner.clone(), &config_with("memory = 'prompt'"));
        assert!(recall
            .inject(&dropped_turns(), "when do we ship?")
            .is_none());
        assert!(inner.calls().is_empty());
    }

    #[test]
    fn no_tool_answer_that_is_not_a_recall_result_ever_reaches_the_model() {
        // Every non-result answer the tool seam can produce: a permission
        // refusal, the contained internal-error notice, a bounded-wait
        // timeout, the unknown-tool string (the memory-unavailable fallback),
        // a raw Debug rendering, and a well-formed result with no hits.
        for answer in [
            text::get("permissions.denied").to_owned(),
            text::get("tools.internal_error").to_owned(),
            format!("{TOOL_RECALL}: {}", text::get("tools.tool_timeout")),
            format!("{TOOL_RECALL}: {}", text::get("tools.memory_tool_failed")),
            text::get("companion.unknown_tool").to_owned(),
            format!("{:?}", ("RecallResult", vec!["debug", "shaped"])),
            result_json(&[], ""),
            String::new(),
        ] {
            let recall = GraphRecall::new(Canned::new(answer.clone()), &Config::default());
            assert!(
                recall
                    .inject(&dropped_turns(), "when do we ship?")
                    .is_none(),
                "injected a non-result answer: {answer}"
            );
        }
    }

    #[test]
    fn an_empty_query_never_reaches_the_tool() {
        let inner = Canned::new(result_json(&["anything"], ""));
        let recall = GraphRecall::new(inner.clone(), &Config::default());
        assert!(recall.inject(&[], "   ").is_none());
        assert!(inner.calls().is_empty());
    }

    #[test]
    fn the_injection_and_the_query_are_both_bounded() {
        // The injection lands in a window that is already under pressure, so
        // an enormous recall block must be clamped here rather than left for
        // `pack_messages` to discard whole.
        let huge = "x".repeat(50_000);
        let inner = Canned::new(result_json(&["hit"], &huge));
        let recall = GraphRecall::new(inner.clone(), &Config::default());
        let long_turn = "z".repeat(40_000);
        let injected = recall
            .inject(&[Message::user("y".repeat(40_000))], &long_turn)
            .expect("a hit must still be injected");
        assert!(
            injected.content.chars().count() <= MAX_INJECTION_CHARS,
            "injection is {} chars",
            injected.content.chars().count()
        );
        assert!(
            crate::companion::estimate_tokens(injected.content.chars().count()) < 300,
            "the injection must stay small against a window under pressure"
        );
        let query = inner.calls()[0]
            .strip_prefix("lambo_recall:")
            .unwrap()
            .to_owned();
        assert!(query.chars().count() <= MAX_QUERY_CHARS, "{}", query.len());
    }

    #[test]
    fn recall_for_chat_shares_the_session_stack_and_opens_no_second_memory() {
        // Constraint 2: `MemoryTools::for_chat` already owns the open Memory.
        // A second open would contend for the single-writer session lease, so
        // the factory must take the composed stack as a parameter and share
        // that Arc — never open anything of its own.
        let production = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap();
        let factory = production
            .split("pub fn recall_for_chat")
            .nth(1)
            .expect("recall_for_chat must exist alongside executor_for_chat")
            .split("\nfn ")
            .next()
            .unwrap();
        assert!(
            factory.contains("tools: Arc<dyn ToolExecutor>"),
            "recall_for_chat must be handed the session's stack: {factory}"
        );
        for opener in [
            "open_memory",
            "MemoryTools::for_chat",
            "crate::memory::open",
        ] {
            assert!(
                !factory.contains(opener),
                "recall_for_chat must not open memory ({opener}): {factory}"
            );
        }
    }

    #[test]
    fn chat_wires_the_production_recall_injector() {
        // `mooshik chat` must hand `run_chat` a REAL injector built over the
        // same tool stack (one open Memory, one single-writer lease). Without
        // this the model only ever sees old context when it decides to call
        // `lambo_recall` itself — the gap this milestone closed.
        //
        // The pin lives here rather than in the CLI's own tests because it is
        // this module's wire that would go missing; `include_str!` reads the
        // caller's source. `cli.rs` became the `cli/` directory module when a
        // config write path pushed it past the 1000-line cap, so the same two
        // functions are now read from `cli/chat_cmd.rs` — same slice markers,
        // same property, same failure when the wire is cut.
        let src = include_str!("../cli/chat_cmd.rs");
        let body = src
            .split("fn chat(")
            .nth(1)
            .expect("cli::chat_cmd::chat must exist")
            .split("fn open_vault_for_chat")
            .next()
            .unwrap();
        assert!(
            body.contains("recall_for_chat(&config, Arc::clone(&executor))"),
            "chat must build the injector over the executor's own Arc: {body}"
        );
        assert!(
            body.contains("run_chat(&config, executor, recall)"),
            "the injector must reach run_chat: {body}"
        );
    }

    #[tokio::test]
    async fn an_unavailable_memory_injects_nothing_and_does_not_crash() {
        // Constraint 4: the product default is Postgres with no DSN, so
        // `for_chat` returns None and `executor_for_chat` falls back to the
        // gated No-op. Chat must still run; recall simply contributes nothing.
        let config = Config::default();
        let stack = super::super::executor_for_chat(&config, None);
        let recall = super::super::recall_for_chat(&config, stack);
        assert!(recall
            .inject(&dropped_turns(), "when do we ship?")
            .is_none());
    }

    #[test]
    fn a_recalled_vault_value_is_redacted_before_it_reaches_the_model() {
        // Sharing the session's composed stack is what buys this: the
        // injection crosses the SAME M6 egress redactor a model-issued
        // `lambo_recall` crosses. A graph that happens to hold a secret must
        // not leak it into the prompt by the recall-injection path.
        const VALUE: &str = "recall-injection-live-value";
        let dir = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-recall-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut vault = crate::vault::Vault::open(
            dir.join("vault"),
            Arc::new(crate::vault::PassphraseProvider::new("pw").unwrap()),
        )
        .unwrap();
        vault.set("token", VALUE).unwrap();
        let vault = crate::vault::Vault::shared(vault);

        let config = Config::default();
        let stack = super::super::compose_chat_stack(
            Canned::new(result_json(&[&format!("the token is {VALUE}")], "")),
            Some(vault),
            config.permissions.grants(),
            None,
            super::super::Diagnostics::stderr(),
        );
        let recall = super::super::recall_for_chat(&config, stack);
        let injected = recall
            .inject(&dropped_turns(), "what is the token?")
            .expect("the hit must still be injected, redacted");
        assert!(!injected.content.contains(VALUE), "{}", injected.content);
        assert!(
            injected.content.contains(crate::tools::redact::REDACTED),
            "{}",
            injected.content
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_real_derive_is_recalled_back_through_the_injector() {
        const MARKER: &str = "mooshik m11 axolotl protocol UNIQUERECALLmarkerWQ";
        let mut config = Config::default();
        config.store.kind = lambo::StoreKind::Memory;
        config.embedder.kind = lambo::EmbedderKind::Fixture;
        config.embedder.dim = 1024;
        config.session.id = "mooshik".to_owned();
        crate::memory::provision(&config).await.unwrap();
        let memory = crate::memory::open(&config).await.unwrap();
        memory
            .derive(
                &[(MARKER, lambo::ConceptType::Entity)],
                &lambo::graph::derive::ParentOf::none(),
            )
            .await
            .unwrap();
        let tools: Arc<dyn ToolExecutor> = Arc::new(MemoryTools::from_memory(memory));
        let recall = super::super::recall_for_chat(&config, Arc::clone(&tools));
        let injected = recall
            .inject(
                &[Message::user("older turn about the axolotl protocol")],
                "remind me about the mooshik m11 axolotl protocol",
            )
            .expect("a derived concept must come back through the injector");
        assert!(
            injected.content.contains("UNIQUERECALLmarkerWQ"),
            "{}",
            injected.content
        );
    }
}
