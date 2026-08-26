use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncBufReadExt;

use crate::config::{CompanionConfig, Config};

use super::cancel::Cancellation;
use super::client::CompanionClient;
use super::session::Session;
use super::tools::ToolExecutor;
use super::CompanionError;

/// Run the interactive chat loop.
///
/// `executor` provides the tool surface (in M4, the lambo tools + scratch
/// runner, or a No-op when memory is unavailable). The chat loop itself never
/// opens Memory — the caller (`crate::cli::chat`) opens and injects it, keeping
/// this module free of any direct reference to the memory module (M3 pin).
pub fn run_chat(config: &Config, executor: Arc<dyn ToolExecutor>) -> Result<(), CompanionError> {
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
        .block_on(run_chat_async(&config.companion, Arc::clone(&executor)));
    drop(executor);
    outcome
}

async fn run_chat_async(
    config: &CompanionConfig,
    executor: Arc<dyn ToolExecutor>,
) -> Result<(), CompanionError> {
    let client = CompanionClient::from_config(config)?;
    let mut session = Session::new(client, config.context_window).with_executor(executor);
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

fn lock(
    current: &Arc<Mutex<Option<Cancellation>>>,
) -> std::sync::MutexGuard<'_, Option<Cancellation>> {
    current.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
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
        let block_on = ".block_on(run_chat_async(&config.companion, Arc::clone(&executor)));";
        assert!(
            body.contains(block_on),
            "the block_on outcome must be bound, not propagated with `?`: {body}"
        );
        let close = body
            .find("drop(executor);")
            .expect("run_chat must drop the executor");
        let loop_end = body.find(block_on).unwrap();
        assert!(
            close > loop_end,
            "the executor close must run after the loop exits, on every path"
        );
    }
}
