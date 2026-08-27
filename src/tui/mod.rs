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
//! complete: every artboard draws from [`model::Workspace`], and `mooshik tui
//! --demo` shows the ported ones with the design's own content — `--demo` is
//! `1a`, `--demo recall` adds `1c`'s quoted words and `--demo caution` adds
//! `1d`'s one careful sentence, because those two artboards are states of the
//! conversation and there is no other way to reach them until the chat loop
//! lands. On the live path, the status bar is real — it comes from
//! [`crate::memory::stats`] — and the rest of the workspace is empty, because the
//! data the artboards show does not exist behind Mooshik yet: a day's weather and
//! mood have no source at all, and per-day thread marks need recalled nodes
//! grouped by event date. Filling those in is a change to [`live`] and to nothing
//! else, which is the reason the screens read the model and never a store.

pub mod app;
pub mod grid;
pub mod input;
pub mod model;
pub mod screen;
pub mod theme;
pub mod widget;
pub mod wrap;

use std::{
    io::{self, IsTerminal},
    time::Duration,
};

use ratatui::crossterm::event::{self, Event};

use crate::{config::Config, memory::MemoryError, text};

use model::{Health, Workspace};

/// The design's own demo day, as a workspace.
const DEMO: &str = include_str!("demo.toml");
/// What artboard `1c` adds to it, and what `1d` does.
const DEMO_RECALL: &str = include_str!("demo_recall.toml");
const DEMO_CAUTION: &str = include_str!("demo_caution.toml");

/// How long to wait for a key before redrawing anyway.
///
/// The screens are static between keystrokes today, so this only needs to be
/// short enough that a resize is picked up promptly. It becomes load-bearing when
/// the companion's stream arrives, at which point a tick is what paints a partial
/// reply.
const TICK: Duration = Duration::from_millis(250);

/// Which of the three Today artboards `--demo` is showing.
///
/// Three scenes rather than three screens, because `1a`, `1c` and `1d` *are* one
/// screen: the recall card and the caution are [`model::Turn`] variants in the
/// same conversation. So a scene is a handful of extra turns layered onto the
/// same workspace, and [`screen::today`] cannot tell it was told anything.
///
/// It exists because `--demo` claimed to show the artboards and could only show
/// one of them: the two that carry the design's whole argument — memory
/// producing something the model forgot, and one careful sentence before the
/// user contradicts themselves — were unreachable without the chat loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scene {
    /// `1a`. The ordinary Thursday.
    #[default]
    Today,
    /// `1c`. The same day, with Monday's own words quoted back inline.
    Recall,
    /// `1d`. The same day, with one careful sentence standing.
    Caution,
}

impl Scene {
    /// The scene `--demo <value>` names, defaulting to [`Scene::Today`].
    ///
    /// An unknown value falls back to the ordinary day rather than failing:
    /// clap's `value_parser` refuses it long before this is reached, so a
    /// stricter contract here would be an error path nothing can produce.
    pub fn named(value: Option<&str>) -> Self {
        match value {
            Some("recall") => Self::Recall,
            Some("caution") => Self::Caution,
            _ => Self::Today,
        }
    }

    /// The fixture layered onto the base day, if this scene adds one.
    const fn overlay(self) -> Option<&'static str> {
        match self {
            Self::Today => None,
            Self::Recall => Some(DEMO_RECALL),
            Self::Caution => Some(DEMO_CAUTION),
        }
    }
}

/// The extra turns a scene adds to the demo day.
///
/// A separate shape rather than a whole second [`Workspace`] per scene: the
/// week, the threads, the trickle and the log are the same Thursday in all
/// three artboards, and duplicating them would mean three files to keep in
/// agreement about a day that only differs in what was said.
#[derive(serde::Deserialize)]
struct Overlay {
    /// The clock in the title rule, which moves on between artboards — `1a` is
    /// 14:22, `1c` 14:31, `1d` 15:03.
    #[serde(default)]
    time: String,
    /// What to append to the conversation.
    turns: Vec<model::Turn>,
    /// Extra lines for Today's log, which the artboards also grow.
    #[serde(default)]
    entries: Vec<model::Entry>,
}

/// The artboards' own content: a Thursday with a postmortem, a novel finished on
/// the train, and an unreturned call to Mum.
///
/// Panics if the embedded fixtures do not parse, which is a programmer error
/// against files compiled into the binary — the same contract as
/// [`text::get`](crate::text::get). `demo_toml_parses` holds it.
pub fn demo(scene: Scene) -> Workspace {
    let mut workspace: Workspace =
        toml::from_str(DEMO).expect("the embedded demo.toml must parse as a Workspace");
    if let Some(source) = scene.overlay() {
        let overlay: Overlay =
            toml::from_str(source).expect("an embedded demo overlay must parse as an Overlay");
        if !overlay.time.is_empty() {
            workspace.now.time = overlay.time;
        }
        workspace.conversation.turns.extend(overlay.turns);
        workspace.today.entries.extend(overlay.entries);
    }
    workspace
}

/// The live workspace: what Mooshik can actually say about right now.
///
/// Only the status bar is filled, and it is filled honestly — `node_count` is
/// how many things are remembered, and a degraded session says so rather than
/// claiming to be keeping up. Everything else is empty on purpose: an empty
/// panel is a true statement about a source that does not exist yet, and putting
/// the demo's Thursday behind `mooshik tui` would be a false one.
pub async fn live(config: &Config) -> Result<Workspace, MemoryError> {
    Ok(from_stats(&crate::memory::stats(config).await?))
}

/// The live workspace's shape, without the database in front of it.
///
/// Split out of [`live`] so the one thing worth pinning about the primary path
/// can be pinned: what the chrome reads when almost every field is empty. See
/// `the_live_chrome_draws_no_dangling_separator`.
fn from_stats(stats: &lambo::MemoryStats) -> Workspace {
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
    Workspace {
        person: text::get("tui.person_unknown").to_owned(),
        health: Health {
            state: state.to_owned(),
            scope: scope.clone(),
            short_scope: scope,
            well,
        },
        ..Workspace::default()
    }
}

/// Take the terminal: raw mode, the alternate screen, and a panic hook that puts
/// both back so a panic inside the loop cannot leave the user's shell without an
/// echo.
///
/// `try_init` rather than `init`, which is `try_init().expect(..)`: `mooshik tui`
/// in a pipe, a cron job or a CI step has no controlling terminal, and the
/// `expect` aborted with a bare panic and a backtrace — past
/// [`Failure::report`](crate::cli::Failure::report), which `crate::cli`'s header
/// states is the only way an error reaches the terminal. The error is returned
/// instead, and [`tui_cmd`](crate::cli) gives it the sentence that explains what
/// a terminal is needed for.
///
/// **Separate from [`run`] so that sentence is attached to this failure and no
/// other.** It used to wrap every error the whole session could produce, so a
/// `terminal.draw` or `event::read` that failed an hour in was reported as "this
/// process has no terminal" — a diagnosis that was true of neither the cause nor
/// the cure.
///
/// `try_init` enables raw mode *before* entering the alternate screen, so a
/// failure at the second step returns with the terminal already in raw mode and
/// the shell left without an echo. `restore` is idempotent, so it is safe to run
/// on a handshake that got part of the way.
///
/// The tty check in front of it is not redundant. `restore` unconditionally
/// writes the leave-alternate-screen sequence, so calling it after a handshake
/// that never entered the alternate screen emits a stray `ESC[?1049l` — harmless
/// in a terminal, which interprets it, but noise in the pipe or log file that a
/// no-tty run is by definition writing to. Refusing before touching the terminal
/// means the one case that is certain to fail this way prints its sentence and
/// nothing else.
pub fn start() -> io::Result<ratatui::DefaultTerminal> {
    refuse_without_a_terminal(io::stdout().is_terminal())?;
    ratatui::try_init().inspect_err(|_| ratatui::restore())
}

/// The tty decision, separated from the tty itself so it can be tested.
///
/// A test binary's stdout is captured, so asking `start` to prove it refuses
/// would only ever exercise the refusing branch — and would start failing the
/// day someone ran the suite with a terminal attached. Taking the answer as an
/// argument tests both branches and neither depends on the harness.
fn refuse_without_a_terminal(is_terminal: bool) -> io::Result<()> {
    if is_terminal {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "standard output is not a terminal",
    ))
}

/// Run the TUI until the user leaves, putting the terminal back either way.
pub fn run(mut terminal: ratatui::DefaultTerminal, workspace: Workspace) -> io::Result<()> {
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

    use model::{Recall, Speaker, Turn};

    /// A run with no terminal is refused before anything is written, so the
    /// sentence explaining why is the only thing that reaches the pipe.
    #[test]
    fn a_run_without_a_terminal_is_refused_before_the_handshake() {
        let refused = refuse_without_a_terminal(false).expect_err("no tty must be refused");
        assert_eq!(refused.kind(), io::ErrorKind::Unsupported);
        assert!(refuse_without_a_terminal(true).is_ok());
    }

    /// The embedded demo must parse — [`demo`] panics otherwise, and it is
    /// reached from a user-facing command.
    #[test]
    fn demo_toml_parses() {
        let workspace = demo(Scene::Today);
        assert_eq!(workspace.person, "Neom");
        assert_eq!(workspace.now.time, "14:22");
    }

    /// The demo carries the artboards' own content, at the shape the screens
    /// expect: seven days, five threads, five trickle lines.
    #[test]
    fn the_demo_matches_the_artboards() {
        let workspace = demo(Scene::Today);
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
        let counts: Vec<usize> = demo(Scene::Today)
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
        for thread in demo(Scene::Today).threads {
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
        let workspace = demo(Scene::Today);
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
        let workspace = demo(Scene::Today);
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

    /// The two extra scenes parse and add exactly what the artboards add — a
    /// recall card in the scroll for `1c`, a standing caution for `1d` — on top of
    /// the same Thursday.
    #[test]
    fn the_scenes_layer_the_two_remaining_artboards_onto_the_same_day() {
        let base = demo(Scene::Today);
        assert!(
            !base
                .conversation
                .turns
                .iter()
                .any(|turn| matches!(turn, Turn::Recalled(_) | Turn::Cautioned(_))),
            "the ordinary day already carries a card"
        );

        let recall = demo(Scene::Recall);
        assert_eq!(recall.week.label, base.week.label, "the week changed");
        assert_eq!(recall.threads.len(), base.threads.len());
        assert_eq!(recall.now.time, "14:31", "`1c`'s clock is 14:31");
        assert!(
            recall.conversation.turns.len() > base.conversation.turns.len(),
            "the scene added nothing"
        );
        let cards: Vec<&Recall> = recall
            .conversation
            .turns
            .iter()
            .filter_map(|turn| match turn {
                Turn::Recalled(card) => Some(card),
                _ => None,
            })
            .collect();
        assert_eq!(cards.len(), 2, "`1c` quotes Monday and Wednesday");
        assert_eq!(cards[0].source, "From Monday 24 August");
        assert!(cards[0].quote.contains("Blocking the writer is honest"));
        assert!(!cards[0].because.is_empty(), "a card with no reason");
        // A recall is not the last word: `1c` ends on the person answering.
        assert!(matches!(
            recall.conversation.turns.last(),
            Some(Turn::Said {
                speaker: Speaker::Person,
                ..
            })
        ));

        let caution = demo(Scene::Caution);
        assert_eq!(caution.now.time, "15:03", "`1d`'s clock is 15:03");
        // Last, so the middle panel swaps: `screen::today`'s `Tail` reads only the
        // final turn, and `1d`'s argument is about the moment a caution stands.
        let Some(Turn::Cautioned(card)) = caution.conversation.turns.last() else {
            panic!("the caution is not the standing turn")
        };
        assert!(card.lead.contains("\"block, never drop\""));
        assert_eq!(card.leaning.len(), 4);
        assert!(card.because.contains("say the word and I'll follow"));
        // And the thread the panel describes is there to describe.
        assert_eq!(caution.threads[0].leaned_on.len(), 8);
    }

    /// `--demo <value>` names a scene, and anything else is the ordinary day.
    #[test]
    fn a_scene_is_named_by_its_value() {
        assert_eq!(Scene::named(None), Scene::Today);
        assert_eq!(Scene::named(Some("today")), Scene::Today);
        assert_eq!(Scene::named(Some("recall")), Scene::Recall);
        assert_eq!(Scene::named(Some("caution")), Scene::Caution);
        // clap's `value_parser` refuses anything else long before this.
        assert_eq!(Scene::named(Some("nonsense")), Scene::Today);
    }

    /// The live workspace's chrome draws no separator with nothing on one side of
    /// it, on any of the three screens.
    ///
    /// This is the first thing an operator sees, and it used to be
    /// `Mooshik  ·    ·  ` on Today and the narrow layout,
    /// `Mooshik  ·  Your week  ·  ` on the week, and ` · 214 things remembered` on
    /// the week's bottom rule — because `now` is empty and `week.label` is empty
    /// and every subject was built with an unconditional `format!`. The empty
    /// fields are honest; the separators around them were not.
    #[test]
    fn the_live_chrome_draws_no_dangling_separator() {
        use crate::tui::{app::App, grid::Grid, screen::chrome::View};
        use ratatui::{buffer::Buffer, layout::Rect};

        let workspace = from_stats(&lambo::MemoryStats {
            session: lambo::SessionId::new("mooshik"),
            agent: lambo::AgentId::new("mooshik"),
            flush_lag: Duration::from_millis(0),
            log_depth: 0,
            flush_depth: 0,
            dead_lettered: 0,
            degraded: false,
            node_count: 214,
            edge_count: 0,
            concept_count: 0,
            canonical_count: 0,
            embedded_concepts: 0,
            epoch: 0,
            daemon_cycles: 0,
            canonization_cycles: 0,
            canonization_failures: 0,
        });
        // The status bar is the one thing that is real, so it is there to find.
        assert_eq!(workspace.health.scope, "214 things remembered");
        assert!(
            workspace.now.long_date.is_empty(),
            "the clock is not live yet"
        );

        let separator = text::get("tui.separator").trim();
        let tight = text::get("tui.separator_tight").trim();
        for (view, width, height) in [
            (View::Today, 120u16, 40u16),
            (View::Today, 80, 24),
            (View::Week, 120, 40),
            (View::Week, 80, 24),
        ] {
            let mut app = App::new(workspace.clone());
            app.view = view;
            let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
            let area = buf.area;
            app.draw(&mut Grid::new(&mut buf, area));

            for row in [0u16, height - 1] {
                let line: String = (0..width)
                    .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
                    .collect();
                let line = line.trim_end();
                assert!(
                    !line.ends_with(separator) && !line.ends_with(tight),
                    "{view:?} at {width}x{height} row {row} trails a separator: {line:?}"
                );
                assert!(
                    !line.contains(&format!("{tight}{tight}")),
                    "{view:?} at {width}x{height} row {row} doubles a separator: {line:?}"
                );
                // Nothing on either rule starts with one either — the week's own
                // rule read " · 214 things remembered".
                assert!(
                    !line.trim_start().starts_with(separator),
                    "{view:?} at {width}x{height} row {row} opens on a separator: {line:?}"
                );
            }
        }
    }

    /// The demo's bars are all drawable, so the ribbon has no holes.
    #[test]
    fn every_demo_bar_draws() {
        for day in demo(Scene::Today).week.days {
            assert!(model::Load::BARS.contains(&day.load.glyph()));
        }
    }
}
