//! The command-line surface: parse argv, dispatch, classify what went wrong.
//!
//! The tree itself lives in [`command`]; each command's body lives beside the
//! subsystem it drives ([`memory_cmd`], [`chat_cmd`], [`tui_cmd`], [`configure`],
//! [`secret`]), and [`render`] holds the terminal output. This file stays the
//! map: what exists, and where argv goes.
//!
//! M7 conventions pinned across this module:
//!
//! * Exit codes: `0` success · `2` user error · `1` internal failure, decided
//!   once in [`Failure`] and documented in `--help`.
//! * Errors reach the terminal through [`Failure::report`] and nowhere else —
//!   top-level message only, never a source chain (see its doc comment).
//! * Every example printed in `--help` parses as written (`cli::tests`).

use std::{env, future::Future};

use anyhow::anyhow;

use crate::{config, home::HomeLayout, memory::MemoryError, text};

mod chat_cmd;
mod command;
mod configure;
mod failure;
mod init_flow;
mod memory_cmd;
mod render;
mod resolve;
mod secret;
mod tui_cmd;
mod watcher;

#[cfg(test)]
mod tests;

pub use command::command;
pub use failure::Failure;

/// Parse argv and dispatch the commands implemented by the current milestones.
///
/// Clap answers usage errors itself (its own help/usage text on stderr, exit
/// code 2), which is the same number [`Failure`] uses for runtime user errors —
/// one convention end to end.
pub fn run() -> Result<(), Failure> {
    let matches = command().get_matches();
    dispatch(&matches).map_err(Failure::from)
}

fn dispatch(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let home = config::resolve_home(env::vars()).map_err(anyhow::Error::new)?;
    let layout = HomeLayout::new(home);
    match matches.subcommand() {
        Some(("init", args)) => memory_cmd::initialize(&layout, args),
        Some(("serve", _)) => memory_cmd::serve(&layout),
        Some(("chat", _)) => chat_cmd::chat(&layout),
        Some(("tui", args)) => tui_cmd::tui(&layout, args),
        Some(("recall", args)) => memory_cmd::recall(&layout, args),
        Some(("stats", _)) => memory_cmd::stats(&layout),
        Some(("reflect", args)) => memory_cmd::reflect(&layout, args),
        Some(("config", sub)) => match sub.subcommand() {
            Some(("show", _)) => configure::show_config(&layout),
            Some(("set", args)) => configure::set_config(&layout, args),
            Some(("coder", args)) => configure::configure_coder(&layout, args),
            _ => Ok(()),
        },
        Some(("configure", sub)) => match sub.subcommand() {
            Some(("coder", args)) => configure::configure_coder(&layout, args),
            _ => Ok(()),
        },
        Some(("permissions", _)) => configure::show_permissions(&layout),
        Some(("secret", sub)) => secret::dispatch_secret(&layout, sub),
        _ => Ok(()),
    }
}

fn block_on<T>(fut: impl Future<Output = Result<T, MemoryError>>) -> anyhow::Result<T> {
    runtime()?.block_on(fut).map_err(anyhow::Error::new)
}

/// A runtime for a command that has to outlive one `block_on`.
///
/// [`block_on`] answers every one-shot: open, do the thing, close, drop. `tui`
/// cannot, because it holds the session open across a synchronous redraw loop —
/// the `Memory` has to survive between the open and the close, and so does the
/// runtime its background tasks are on.
fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| anyhow!(text::get("memory.runtime_failed")))
}
