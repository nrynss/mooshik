//! The TUI (M11) — the intended face of the product.
//!
//! Ported from `scratch_design/Mooshik TUI.dc.html`, whose nine artboards are a
//! character-grid spec rather than a mockup: every panel carries `col`/`row`/`w`
//! /`h` in cells and every text run is placed at an explicit column and row. The
//! module layout follows that:
//!
//! | Module     | What it owns                                              |
//! | ---------- | --------------------------------------------------------- |
//! | [`theme`]  | the 16-colour palette and what each colour means (`1i`)   |
//! | [`model`]  | the plain-data tree the screens are a pure function of    |
//! | [`grid`]   | absolute character placement in the design's coordinates  |
//! | [`wrap`]   | word wrapping, because prose renders at three widths      |
//! | [`widget`] | the frame component and the strength notations            |
//! | [`screen`] | one module per artboard                                   |
//! | [`app`]    | what is showing, and what a key does to it                |
//! | [`input`]  | keys to actions — only the bindings the artboards print   |
//!
//! **What is wired, and what is not.** The render and interaction layers are
//! complete: every artboard draws from [`model::Workspace`], and
//! `mooshik tui --demo` shows them with the design's own content. On the live
//! path, the status bar is real — it comes from [`crate::memory::stats`] — and
//! the rest of the workspace is empty, because the data the artboards show does
//! not exist behind Mooshik yet: a day's weather and mood have no source at all,
//! and per-day thread marks need recalled nodes grouped by event date. Filling
//! those in is a change to [`live`] and to nothing else, which is the reason the
//! screens read the model and never a store.

pub mod app;
pub mod grid;
pub mod input;
pub mod model;
pub mod screen;
pub mod theme;
pub mod widget;
pub mod wrap;

use std::{io, time::Duration};

use ratatui::crossterm::event::{self, Event};

use crate::{config::Config, memory::MemoryError, text};

use model::{Health, Workspace};

/// The design's own demo day, as a workspace.
const DEMO: &str = include_str!("demo.toml");

/// How long to wait for a key before redrawing anyway.
///
/// The screens are static between keystrokes today, so this only needs to be
/// short enough that a resize is picked up promptly. It becomes load-bearing when
/// the companion's stream arrives, at which point a tick is what paints a partial
/// reply.
const TICK: Duration = Duration::from_millis(250);

/// The artboards' own content: a Thursday with a postmortem, a novel finished on
/// the train, and an unreturned call to Mum.
///
/// Panics if `demo.toml` does not parse, which is a programmer error against a
/// file compiled into the binary — the same contract as
/// [`text::get`](crate::text::get). `demo_toml_parses` holds it.
pub fn demo() -> Workspace {
    toml::from_str(DEMO).expect("the embedded demo.toml must parse as a Workspace")
}

/// The live workspace: what Mooshik can actually say about right now.
///
/// Only the status bar is filled, and it is filled honestly — `node_count` is
/// how many things are remembered, and a degraded session says so rather than
/// claiming to be keeping up. Everything else is empty on purpose: an empty
/// panel is a true statement about a source that does not exist yet, and putting
/// the demo's Thursday behind `mooshik tui` would be a false one.
pub async fn live(config: &Config) -> Result<Workspace, MemoryError> {
    let stats = crate::memory::stats(config).await?;
    // Two words at most, per `1i`: "One mark, one word: reachable, saved,
    // keeping up. Never a sentence."
    let (state, well) = if stats.degraded {
        (text::get("tui.health_degraded"), false)
    } else if stats.log_depth > 0 {
        (text::get("tui.health_catching_up"), false)
    } else {
        (text::get("tui.health_keeping_up"), true)
    };
    let scope = text::get("tui.scope_live").replace("{count}", &stats.node_count.to_string());
    Ok(Workspace {
        person: text::get("tui.person_unknown").to_owned(),
        health: Health {
            state: state.to_owned(),
            scope: scope.clone(),
            short_scope: scope,
            well,
        },
        ..Workspace::default()
    })
}

/// Run the TUI until the user leaves.
///
/// `ratatui::init` takes the terminal into raw mode and the alternate screen and
/// installs a panic hook that puts it back, so a panic inside the loop cannot
/// leave the user's shell without an echo.
pub fn run(workspace: Workspace) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, workspace);
    ratatui::restore();
    result
}

/// The draw-and-read loop, separated from the terminal handshake so a failure
/// inside it still runs [`ratatui::restore`].
fn event_loop(terminal: &mut ratatui::DefaultTerminal, workspace: Workspace) -> io::Result<()> {
    let mut app = app::App::new(workspace);
    while app.running {
        terminal.draw(|frame| {
            let area = frame.area();
            let mut screen = grid::Grid::new(frame.buffer_mut(), area);
            app.draw(&mut screen);
        })?;

        if !event::poll(TICK)? {
            continue;
        }
        // Only keys carry actions. A resize needs no handling of its own: the
        // next draw reads `frame.area()` and the screens derive their whole
        // layout from it, which is what the grid's clipping is for.
        if let Event::Key(key) = event::read()? {
            let action = input::action(key, app.is_typing());
            app.apply(action);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use model::{Speaker, Turn};

    /// The embedded demo must parse — [`demo`] panics otherwise, and it is
    /// reached from a user-facing command.
    #[test]
    fn demo_toml_parses() {
        let workspace = demo();
        assert_eq!(workspace.person, "Neom");
        assert_eq!(workspace.now.time, "14:22");
    }

    /// The demo carries the artboards' own content, at the shape the screens
    /// expect: seven days, five threads, five trickle lines.
    #[test]
    fn the_demo_matches_the_artboards() {
        let workspace = demo();
        assert_eq!(workspace.week.days.len(), 7, "the week is seven days");
        assert_eq!(workspace.threads.len(), 5);
        assert_eq!(workspace.trickle.len(), 5);
        assert_eq!(workspace.week.label, "21-27 August");
        // Wednesday is the hard day, and the one the week screen opens on.
        assert_eq!(workspace.week.selected, 5);
        let wednesday = &workspace.week.days[5];
        assert_eq!(wednesday.short_label, "Wed 26");
        assert!(matches!(
            wednesday.mood.as_ref().map(|m| m.tone),
            Some(model::Tone::Hard)
        ));
    }

    /// The demo's threads are ordered strongest first, which is the encoding the
    /// renderer depends on — nothing sorts them at draw time.
    #[test]
    fn the_demo_threads_are_already_ordered() {
        let counts: Vec<usize> = demo()
            .threads
            .iter()
            .map(model::Thread::day_count)
            .collect();
        assert_eq!(counts, [7, 5, 5, 3, 2], "{counts:?}");
        assert!(
            counts.windows(2).all(|pair| pair[0] >= pair[1]),
            "the demo's threads are not in order"
        );
    }

    /// Every thread has a reason, because the week screen draws all of them.
    #[test]
    fn every_demo_thread_has_a_reason() {
        for thread in demo().threads {
            assert!(
                !thread.because.is_empty(),
                "{:?} has no reason",
                thread.summary
            );
        }
    }

    /// The conversation alternates as the artboard does, and its prose is
    /// unwrapped — a hard-wrapped fixture would break at the wrong column on
    /// every panel that is not exactly the artboard's width.
    #[test]
    fn the_demo_conversation_is_unwrapped_prose() {
        let workspace = demo();
        assert_eq!(workspace.conversation.turns.len(), 8);
        assert!(matches!(
            workspace.conversation.turns.first(),
            Some(Turn::Said {
                speaker: Speaker::Person,
                ..
            })
        ));
        for turn in &workspace.conversation.turns {
            if let Turn::Said { text, .. } = turn {
                assert!(!text.contains('\n'), "{text:?} is hand-wrapped");
            }
        }
    }

    /// Every day of the demo week has a gutter summary, so no week column is
    /// blank; Wednesday additionally has the timed log its detail pane shows.
    #[test]
    fn every_demo_day_fills_its_column() {
        let workspace = demo();
        for day in &workspace.week.days {
            assert!(
                !day.highlights.is_empty(),
                "{} has nothing in its column",
                day.short_label
            );
            assert!(!day.short_label.is_empty());
            assert!(day.weather.is_some());
            assert!(day.mood.is_some());
        }
        let wednesday = &workspace.week.days[5];
        assert!(!wednesday.entries.is_empty(), "Wednesday has no log");
        assert!(wednesday.detail_entries()[0].time.is_some());
    }

    /// The demo's bars are all drawable, so the ribbon has no holes.
    #[test]
    fn every_demo_bar_draws() {
        for day in demo().week.days {
            assert!(model::Load::BARS.contains(&day.load.glyph()));
        }
    }
}
