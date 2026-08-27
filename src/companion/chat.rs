use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncBufReadExt;

use crate::config::{CompanionConfig, Config};

use super::cancel::Cancellation;
use super::client::CompanionClient;
use super::pack::RecallInjector;
use super::session::Session;
use super::tools::ToolExecutor;
use super::CompanionError;

/// Run the interactive chat loop.
///
/// `executor` provides the tool surface (in M4, the lambo tools + scratch
/// runner, or a No-op when memory is unavailable) and `recall` the injector
/// that brings dropped turns back as recalled memory. The chat loop itself
/// never opens Memory — the caller (`crate::cli::chat`) opens it once and
/// injects both, keeping this module free of any direct reference to the
/// memory module (M3 pin).
pub fn run_chat(
    config: &Config,
    executor: Arc<dyn ToolExecutor>,
    recall: Arc<dyn RecallInjector>,
) -> Result<(), CompanionError> {
    // The caller's `executor` handle outlives `block_on` here on purpose: the
    // session's clone dies inside the async context, and the last reference
    // drops only after the runtime is gone — so a memory-backed executor can
    // run its graceful close (`Runtime::block_on` in `Drop`) legally.
    //
    // The outcome is bound first so the executor closes on the FAILURE path
    // too: a classified-failure exit must not skip the graceful close, or the
    // single-writer lease is held until its TTL lapses and the write-behind
    // tail is lost like a crash.
    let outcome = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| CompanionError::Runtime)?
        .block_on(run_chat_async(
            &config.companion,
            Arc::clone(&executor),
            Arc::clone(&recall),
        ));
    // `recall` holds its own clone of the tool stack, so it is released FIRST
    // and `executor` stays the last reference: the graceful close still runs
    // here, on this plain thread, after the runtime is gone.
    drop(recall);
    drop(executor);
    outcome
}

async fn run_chat_async(
    config: &CompanionConfig,
    executor: Arc<dyn ToolExecutor>,
    recall: Arc<dyn RecallInjector>,
) -> Result<(), CompanionError> {
    let client = CompanionClient::from_config(config)?;
    let mut session = compose_session(client, config.context_window, executor, recall);
    let shutdown = Cancellation::new();
    let current: Arc<Mutex<Option<Cancellation>>> = Arc::new(Mutex::new(None));
    tokio::spawn({
        let shutdown = shutdown.clone();
        let current = current.clone();
        async move {
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    shutdown.cancel();
                    break;
                }
                let guard = current.lock().unwrap_or_else(|error| error.into_inner());
                match guard.as_ref() {
                    Some(cancel) if !cancel.is_cancelled() => cancel.cancel(),
                    _ => {
                        shutdown.cancel();
                        break;
                    }
                }
            }
        }
    });

    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(line)) => line,
                    Ok(None) => return Ok(()),
                    Err(_) => return Err(CompanionError::Io),
                };
                if line.trim().is_empty() {
                    continue;
                }
                let cancel = Cancellation::new();
                *lock(&current) = Some(cancel.clone());
                let result = session
                    .turn(&line, &cancel, |token| {
                        let mut out = io::stdout();
                        let _ = out.write_all(token.as_bytes());
                        let _ = out.flush();
                    })
                    .await;
                *lock(&current) = None;
                match result {
                    Ok(_) | Err(CompanionError::Cancelled) => println!(),
                    Err(error) => eprintln!("{error}"),
                }
            }
        }
    }
}

/// The production session composition: the tool surface AND the recall
/// injector, both installed on every chat session.
///
/// Extracted as a named seam (the same technique as
/// `crate::tools::compose_chat_stack`) so that dropping `.with_recall(...)` —
/// the wire that makes turns dropped for context pressure come back as
/// recalled memory instead of vanishing — is caught by *driving* this
/// function, not only by reading its source.
fn compose_session(
    client: CompanionClient,
    window: u32,
    executor: Arc<dyn ToolExecutor>,
    recall: Arc<dyn RecallInjector>,
) -> Session<Arc<dyn ToolExecutor>, Arc<dyn RecallInjector>> {
    Session::new(client, window)
        .with_executor(executor)
        .with_recall(recall)
}

fn lock(
    current: &Arc<Mutex<Option<Cancellation>>>,
) -> std::sync::MutexGuard<'_, Option<Cancellation>> {
    current.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::mock::{Frame, MockServer, Script};
    use super::super::pack::message_tokens;
    use super::super::types::Message;
    use super::super::NoopExecutor;
    use super::*;

    /// A recall injector that answers with a marker no turn ever contains.
    struct Marker;

    impl RecallInjector for Marker {
        fn inject(&self, dropped: &[Message], current_user: &str) -> Option<Message> {
            assert!(!dropped.is_empty(), "injected without dropped turns");
            assert_eq!(current_user, "now-please");
            Some(Message::system("PRODUCTION_RECALL_MARKER"))
        }
    }

    /// The M11 wiring pin, behavioral half: production composes its session
    /// through `compose_session`, and that seam must install the injector.
    /// Reverting to `Session::new(...).with_executor(...)` alone — the state
    /// this milestone found — leaves `NoopRecall` in place and the marker
    /// never reaches the request.
    #[tokio::test]
    async fn the_production_session_composition_installs_a_real_recall_injector() {
        let server = MockServer::spawn(vec![Script::sse(vec![
            Frame::content_openai("ok"),
            Frame::finish("stop"),
            Frame::done(),
        ])])
        .await;
        let companion = CompanionConfig {
            base_url: server.base_url.clone(),
            ..CompanionConfig::default()
        };
        let client = CompanionClient::from_config(&companion).unwrap();
        let system = Message::system(crate::text::get("companion.system_prompt"));
        let current = Message::user("now-please");
        let marker = Message::system("PRODUCTION_RECALL_MARKER");
        let window =
            (message_tokens(&system) + message_tokens(&current) + message_tokens(&marker)) as u32;
        let mut session = compose_session(
            client,
            window,
            Arc::new(NoopExecutor) as Arc<dyn ToolExecutor>,
            Arc::new(Marker) as Arc<dyn RecallInjector>,
        );
        session.seed([
            Message::user("UNIQUE_OLD_TURN_xyz"),
            Message::assistant("old-reply", Vec::new()),
        ]);
        session
            .turn("now-please", &Cancellation::new(), |_| {})
            .await
            .unwrap();
        let body = &server.requests()[0].body;
        assert!(body.contains("PRODUCTION_RECALL_MARKER"), "{body}");
        assert!(!body.contains("UNIQUE_OLD_TURN_xyz"), "{body}");
        assert!(!body.contains("old-reply"), "{body}");
        server.assert_all_streaming();
    }

    #[test]
    fn run_chat_async_builds_its_session_through_the_composition_seam() {
        // Structural half: the loop must not hand-roll a second composition
        // that quietly omits the injector.
        let src = include_str!("chat.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        let body = production
            .split("async fn run_chat_async")
            .nth(1)
            .unwrap()
            .split("\nfn compose_session")
            .next()
            .unwrap();
        assert!(
            body.contains("compose_session(client, config.context_window"),
            "run_chat_async must build its session through compose_session: {body}"
        );
        let seam = production
            .split("fn compose_session")
            .nth(1)
            .expect("compose_session must exist")
            .split("\nfn lock")
            .next()
            .unwrap();
        assert!(
            seam.contains(".with_recall(recall)"),
            "the composition must install the recall injector: {seam}"
        );
        assert!(
            !production.contains("NoopRecall"),
            "production chat must never fall back to NoopRecall: {production}"
        );
    }

    #[test]
    fn run_chat_does_not_open_memory() {
        let src = include_str!("chat.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("memory::"), "{production}");
        assert!(!production.contains("crate::memory"), "{production}");
        assert!(
            production.contains("CompanionClient::from_config"),
            "{production}"
        );
    }

    #[test]
    fn run_chat_closes_the_executor_on_the_failure_path_too() {
        // P2-c (honesty half): the block_on outcome must be bound, then the
        // executor dropped UNCONDITIONALLY, before the outcome returns. A `?`
        // on the block_on line would skip the explicit close on the failure
        // path — lease held to TTL, write-behind tail lost like a crash.
        let src = include_str!("chat.rs");
        let body = src
            .split("pub fn run_chat")
            .nth(1)
            .unwrap()
            .split("\nasync fn run_chat_async")
            .next()
            .unwrap();
        let block_on = ".block_on(run_chat_async(";
        let loop_start = body
            .find(block_on)
            .unwrap_or_else(|| panic!("run_chat must block_on the loop: {body}"));
        assert!(
            body[..loop_start].contains("let outcome ="),
            "the block_on outcome must be BOUND, not propagated: {body}"
        );
        let statement = &body[loop_start
            ..loop_start
                + body[loop_start..]
                    .find(';')
                    .expect("the block_on statement must terminate")];
        assert!(
            !statement.contains('?'),
            "a `?` on the block_on statement skips the explicit close on the \
             failure path: {statement}"
        );
        assert!(
            statement.contains("Arc::clone(&executor)"),
            "the loop must take a CLONE, keeping the caller's handle alive as \
             the last reference: {statement}"
        );
        let close = body
            .find("drop(executor);")
            .expect("run_chat must drop the executor");
        let loop_end = body.find(block_on).unwrap();
        assert!(
            close > loop_end,
            "the executor close must run after the loop exits, on every path"
        );
        // M11: the recall injector holds its own clone of the tool stack, so
        // it must be released BEFORE the executor — otherwise the injector's
        // clone is the last reference and it dies inside the async context,
        // where `Memory::close`'s `block_on` is illegal.
        let recall_close = body
            .find("drop(recall);")
            .expect("run_chat must drop the recall injector");
        assert!(
            recall_close > loop_end && recall_close < close,
            "the recall injector must be released after the loop and before \
             the executor: {body}"
        );
    }
}
