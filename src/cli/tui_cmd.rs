//! `mooshik tui` — M11's terminal interface, M12a's data.
//!
//! Two ways in, and the difference matters:
//!
//! * `--demo` draws the design's own Thursday. It touches nothing — no config,
//!   no database, no credentials — so the artboards can be seen on a machine
//!   that has never run `mooshik init`. It takes an optional scene: `--demo
//!   recall` adds `1c`'s quoted words and `--demo caution` adds `1d`'s one
//!   careful sentence, because both artboards are states of the conversation and
//!   nothing else can reach them until the chat loop lands.
//! * Without it, the workspace is the graph — [`crate::memory::view`] reads
//!   the open session into the same view model the artboards are drawn from,
//!   and M12b's tick reads it again: the loop rebuilds on every 250 ms tick,
//!   so a write from the ingester, an MCP client or the reflect pass appears
//!   in the pane without a keystroke.
//!
//! Both paths hand a [`Workspace`](crate::tui::model::Workspace) to
//! `crate::tui::run`; the live path also hands it the rebuild — a closure
//! that answers with the graph as of now — and nothing else, which is what
//! keeps the screens a pure function of the model.
//!
//! **The live path is an ordinary holder of the single-writer lease, exactly as
//! `chat` is.** It takes it for the length of the session and gives it back on
//! the way out, so a `mooshik tui` left open in a tmux split is a writer another
//! process can see and be refused by — with Mooshik's own conflict sentence,
//! which names the holder and no override or page this product does not ship.
//! That refusal is a user error and leaves with exit code 2. There is no
//! read-only or proxied second view; a workspace served through somebody
//! else's lease is a different design and not this one.
//!
//! The session is closed **after** the terminal is put back, on every way out of
//! the loop. Closing is what makes the write-behind tail durable, so `Esc`, a
//! failed draw, a broken pipe and — since M12a's first review round —
//! `SIGTERM`/`SIGHUP` all reach it; the last of those used to kill the process
//! where it stood, which left the alternate screen up and the lease held for its
//! whole TTL by a pid that no longer existed. `crate::tui::run` takes both
//! signals for the length of the session and ends the loop instead. Close's own
//! failure is reported rather than swallowed, because a session that would not
//! close is the one thing here that can lose what was remembered.
//!
//! **A panic is the exception, deliberately.** The panic hook puts the terminal
//! back, the unwind drops the `Memory`, and Lambo's `Drop` aborts the heartbeat
//! without releasing the lease — a handle dropped without a clean close is the
//! crash-shaped path, and the lease is what makes a crash safe. So a panic
//! leaves the lease to lapse on its own TTL, which is the correct answer for a
//! crash and the wrong one for a closed window; only the second is a signal.

use crate::{home::HomeLayout, text, tui::Scene};

use super::{resolve, runtime};

pub(crate) fn tui(layout: &HomeLayout, args: &clap::ArgMatches) -> anyhow::Result<()> {
    match args.get_one::<String>("demo").map(String::as_str) {
        // `--demo` opens no database and rebuilds nothing: its workspace is
        // the design's own Thursday, fixed for the life of the loop.
        Some(scene) => draw(crate::tui::demo(Scene::named(Some(scene))), None),
        None => live(layout),
    }
}

/// The live session: open the graph, draw it — rebuilding the view on every
/// tick — and put both back.
fn live(layout: &HomeLayout) -> anyhow::Result<()> {
    // Same resolution as `mooshik stats`, because the store DSN may be a vault
    // reference and running on an unresolved one would show another database's
    // memory.
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = resolve::load_with_secrets(layout, &root)?;

    // The runtime outlives the loop: `memory` is declared after it and so is
    // dropped before it, which is what lets its background tasks be shut down on
    // a runtime that is still there to shut them down on.
    let runtime = runtime()?;
    let memory = runtime
        .block_on(crate::memory::open(&config))
        .map_err(anyhow::Error::new)?;

    // The first model is built here, and the loop builds another on every
    // quiet tick: `refresh` is the whole seam between the terminal and the
    // graph — the loop asks, it answers with the graph as of now, and nothing
    // in `crate::tui` knows a database exists. A write from anywhere else
    // shows up on the next tick without a keystroke.
    let workspace = crate::memory::view::of_memory(&memory, chrono::Local::now());
    let mut refresh = || crate::memory::view::of_memory(&memory, chrono::Local::now());
    let drawn = draw(workspace, Some(&mut refresh));

    // Both outcomes, in the order they happened: a session that failed to close
    // may have lost the tail of what it remembered, which is worse than a draw
    // that failed, so it is reported when the drawing succeeded.
    let closed = runtime.block_on(memory.close());
    drawn?;
    closed.map_err(crate::memory::MemoryError::from)?;
    Ok(())
}

/// Take the terminal, run the loop, and give the terminal back.
///
/// `refresh` is the live path's rebuild seam; `--demo` passes `None` and its
/// fixed workspace is never rebuilt.
fn draw(
    workspace: crate::tui::model::Workspace,
    refresh: Option<&mut dyn FnMut() -> crate::tui::model::Workspace>,
) -> anyhow::Result<()> {
    // The context, not the `io::Error`: taking a terminal that is not there
    // fails with "Device not configured", which says nothing about what to do.
    // `Failure::rendered` prints the top-level `Display` only, so this sentence
    // is what the operator sees.
    //
    // Two calls, two sentences. It used to be one, and "this process has no
    // terminal" was then attached to every error the whole session could return —
    // including a `terminal.draw`, `event::poll` or `event::read` that failed an
    // hour in, on a terminal that plainly existed. A diagnosis has to be about
    // the failure it is printed under, so the handshake and the loop are separate
    // calls with separate contexts.
    let terminal = crate::tui::start()
        .map_err(|error| anyhow::Error::new(error).context(text::get("tui.needs_a_terminal")))?;
    crate::tui::run(terminal, workspace, refresh)
        .map_err(|error| anyhow::Error::new(error).context(text::get("tui.session_failed")))
}
