//! `mooshik tui` — M11's terminal interface.
//!
//! Two ways in, and the difference matters:
//!
//! * `--demo` draws the design's own Thursday. It touches nothing — no config,
//!   no database, no credentials — so the artboards can be seen on a machine
//!   that has never run `mooshik init`. It takes an optional scene: `--demo
//!   recall` adds `1c`'s quoted words and `--demo caution` adds `1d`'s one
//!   careful sentence, because both artboards are states of the conversation and
//!   nothing else can reach them until the chat loop lands.
//! * Without it, the workspace is live. Today that means the status bar is real
//!   and the panels are empty, because the data the artboards show has no source
//!   behind Mooshik yet; see [`crate::tui`] for exactly which parts and why.
//!
//! Both paths hand a [`Workspace`](crate::tui::model::Workspace) to
//! `crate::tui::run` and nothing else, which is what keeps the screens a pure
//! function of the model.

use crate::{home::HomeLayout, text, tui::Scene};

use super::{block_on, resolve};

pub(crate) fn tui(layout: &HomeLayout, args: &clap::ArgMatches) -> anyhow::Result<()> {
    let scene = args.get_one::<String>("demo").map(String::as_str);
    let workspace = if let Some(scene) = scene {
        crate::tui::demo(Scene::named(Some(scene)))
    } else {
        // Same resolution as `mooshik stats`, because that is where the live
        // status bar comes from: the store DSN may be a vault reference, and
        // running on an unresolved one would report another database's figures.
        let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
        let config = resolve::load_with_secrets(layout, &root)?;
        block_on(crate::tui::live(&config))?
    };
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
    crate::tui::run(terminal, workspace)
        .map_err(|error| anyhow::Error::new(error).context(text::get("tui.session_failed")))
}
