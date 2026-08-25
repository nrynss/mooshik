use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tokio::io::AsyncBufReadExt;

use crate::config::{CompanionConfig, Config};

use super::cancel::Cancellation;
use super::client::CompanionClient;
use super::session::Session;
use super::CompanionError;

pub fn run_chat(config: &Config) -> Result<(), CompanionError> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| CompanionError::Runtime)?
        .block_on(run_chat_async(&config.companion))
}

async fn run_chat_async(config: &CompanionConfig) -> Result<(), CompanionError> {
    let client = CompanionClient::from_config(config)?;
    let mut session = Session::new(client, config.context_window);
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
}
