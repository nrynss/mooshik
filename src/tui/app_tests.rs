//! Tests for [`super`] — the TUI's state and its transitions.
//!
//! A sibling file rather than an inline module so `app.rs` stays inside the
//! crate's ~600-line soft target; `screen/{aside,conversation,week}_tests.rs`
//! follow the same pattern.

use super::*;
use ratatui::{buffer::Buffer, layout::Rect};

use crate::tui::model::{Day, Thread, Week};

fn app() -> App {
    App::new(Workspace {
        threads: (0..5).map(|_| Thread::default()).collect(),
        week: Week {
            days: (0..7).map(|_| Day::default()).collect(),
            selected: 5,
            ..Week::default()
        },
        ..Workspace::default()
    })
}

/// Tab cycles the four panels and comes back round.
#[test]
fn tab_cycles_the_panels() {
    let mut app = app();
    assert_eq!(app.focus, Focus::Conversation);
    for expected in [
        Focus::Today,
        Focus::Threads,
        Focus::Trickle,
        Focus::Conversation,
    ] {
        app.apply(Action::NextPanel);
        assert_eq!(app.focus, expected);
    }
    app.apply(Action::PreviousPanel);
    assert_eq!(app.focus, Focus::Trickle);
}

/// `Tab` is a no-op on the week screen, which draws no focusable panel and
/// never reads [`App::focus`].
///
/// It used to cycle all four variants there, so `Tab` on `1b` moved a field
/// nothing on screen could show — and `J`/`K`, whose cursor that screen *does*
/// draw, are the keys its own bottom rule offers.
#[test]
fn tab_does_nothing_on_the_week_screen() {
    let mut app = app();
    app.apply(Action::ShowWeek);
    for _ in 0..5 {
        app.apply(Action::NextPanel);
        assert_eq!(
            app.focus,
            Focus::Conversation,
            "Tab moved focus on the week"
        );
    }
    app.apply(Action::PreviousPanel);
    assert_eq!(app.focus, Focus::Conversation);
    // And coming back to Today finds the cycle working again.
    app.apply(Action::ShowToday);
    app.apply(Action::NextPanel);
    assert_eq!(app.focus, Focus::Today);
}

/// `Tab` on the narrow layout reaches only the panels `1h` draws — the
/// conversation and the composer, which share one focus — so it moves nothing
/// and typing keeps working.
///
/// One press on an 80x24 terminal used to leave the screen with no accented
/// rule anywhere, keystrokes no longer reaching the draft, and `j`/`k`/`h`/`l`
/// moving two cursors nothing drew.
#[test]
fn tab_on_the_narrow_layout_keeps_the_conversation_focused() {
    let mut app = app();
    // The width comes from the last draw, so draw first.
    screen(&mut app, 80, 24);
    for press in 0..5 {
        app.apply(Action::NextPanel);
        assert_eq!(app.focus(), Focus::Conversation, "press {press}");
        assert!(app.is_typing(), "typing stopped after press {press}");
    }
    // And the keystroke reaches the draft, not the cursors.
    app.apply(Action::Type('x'));
    assert_eq!(app.workspace.conversation.composer.draft, "x");
    assert_eq!(app.thread_cursor, 0);

    // Every panel rule the narrow screen draws is still accented.
    let text = screen(&mut app, 80, 24);
    assert!(text.contains("The conversation"), "{text}");
}

/// A terminal narrowed out of the wide layout drops focus back to the
/// conversation rather than leaving it on a panel the narrow screen does not
/// draw — the same fault `Tab` used to cause, reached by resizing.
#[test]
fn narrowing_the_terminal_brings_focus_back_to_the_conversation() {
    let mut app = app();
    screen(&mut app, 120, 40);
    app.apply(Action::NextPanel);
    app.apply(Action::NextPanel);
    assert_eq!(app.focus, Focus::Threads);
    assert!(!app.is_typing());

    screen(&mut app, 80, 24);
    assert_eq!(app.focus(), Focus::Conversation);
    assert!(app.is_typing(), "typing is dead on the narrow layout");
    // The field keeps the wide screen's choice, so widening finds it again.
    assert_eq!(app.focus, Focus::Threads);
    screen(&mut app, 120, 40);
    assert_eq!(app.focus(), Focus::Threads);
}

/// The thread cursor clamps at both ends rather than wrapping, so it cannot
/// jump from the faintest thing to the strongest.
#[test]
fn the_thread_cursor_clamps_rather_than_wrapping() {
    let mut app = app();
    for _ in 0..10 {
        app.apply(Action::Next);
    }
    assert_eq!(app.thread_cursor, 4, "the cursor ran past the list");
    for _ in 0..10 {
        app.apply(Action::Previous);
    }
    assert_eq!(app.thread_cursor, 0, "the cursor ran past the top");
}

/// The cursor never reorders the list — the point of this module's note.
#[test]
fn the_cursor_never_reorders_the_list() {
    let mut app = app();
    app.workspace.threads = (0..3)
        .map(|n| Thread {
            summary: format!("Thought {n}"),
            ..Thread::default()
        })
        .collect();
    let before: Vec<String> = app
        .workspace
        .threads
        .iter()
        .map(|t| t.summary.clone())
        .collect();
    for _ in 0..5 {
        app.apply(Action::Next);
    }
    app.apply(Action::Previous);
    let after: Vec<String> = app
        .workspace
        .threads
        .iter()
        .map(|t| t.summary.clone())
        .collect();
    assert_eq!(before, after);
}

/// The day cursor clamps to the week.
#[test]
fn the_day_cursor_clamps_to_the_week() {
    let mut app = app();
    app.apply(Action::Right);
    assert_eq!(app.workspace.week.selected, 6);
    app.apply(Action::Right);
    assert_eq!(app.workspace.week.selected, 6);
    for _ in 0..10 {
        app.apply(Action::Left);
    }
    assert_eq!(app.workspace.week.selected, 0);
}

/// An empty list leaves the cursor at zero rather than underflowing.
#[test]
fn empty_lists_do_not_underflow() {
    let mut app = App::new(Workspace::default());
    app.apply(Action::Next);
    app.apply(Action::Previous);
    app.apply(Action::Left);
    app.apply(Action::Right);
    assert_eq!(app.thread_cursor, 0);
    assert_eq!(app.workspace.week.selected, 0);
}

/// Typing appends and backspace removes a whole character, not a byte.
#[test]
fn backspace_removes_a_character_not_a_byte() {
    let mut app = app();
    for character in "the 512 cap —".chars() {
        app.apply(Action::Type(character));
    }
    assert_eq!(app.workspace.conversation.composer.draft, "the 512 cap —");
    app.apply(Action::Backspace);
    assert_eq!(app.workspace.conversation.composer.draft, "the 512 cap ");
    // And the string is still valid UTF-8 that can be drawn.
    assert!(app
        .workspace
        .conversation
        .composer
        .draft
        .is_char_boundary(app.workspace.conversation.composer.draft.len()));
}

/// Backspace on an empty draft is a no-op, not a panic.
#[test]
fn backspace_on_an_empty_draft_does_nothing() {
    let mut app = app();
    app.apply(Action::Backspace);
    assert!(app.workspace.conversation.composer.draft.is_empty());
}

/// `Enter` does not destroy the draft. It cleared it, which read as a send
/// that never happened: the words vanished, no turn was added, and nothing
/// was sent. Until the chat loop is wired the key is inert.
#[test]
fn sending_does_not_discard_the_draft() {
    let mut app = app();
    for character in "Called Mum. No answer.".chars() {
        app.apply(Action::Type(character));
    }
    app.apply(Action::Send);
    assert_eq!(
        app.workspace.conversation.composer.draft, "Called Mum. No answer.",
        "the draft was destroyed by a key that sent nothing"
    );
    // And no turn appeared, because nothing was sent.
    assert!(app.workspace.conversation.turns.is_empty());
}

/// Quitting stops the loop.
#[test]
fn quitting_stops_the_loop() {
    let mut app = app();
    assert!(app.running);
    app.apply(Action::Quit);
    assert!(!app.running);
}

/// Switching views does not disturb focus or either cursor, so coming back
/// finds the screen as it was left.
#[test]
fn switching_views_preserves_the_cursors() {
    let mut app = app();
    app.apply(Action::NextPanel);
    app.apply(Action::Next);
    app.apply(Action::ShowWeek);
    assert_eq!(app.view, View::Week);
    app.apply(Action::ShowToday);
    assert_eq!(app.view, View::Today);
    assert_eq!(app.focus, Focus::Today);
    assert_eq!(app.thread_cursor, 1);
}

/// Typing only reaches the draft when the conversation has focus on the
/// Today screen; elsewhere the navigation keys own the letters.
#[test]
fn typing_is_scoped_to_the_focused_conversation() {
    let mut app = app();
    assert!(app.is_typing());
    app.apply(Action::NextPanel);
    assert!(!app.is_typing());
    app.apply(Action::PreviousPanel);
    assert!(app.is_typing());
    app.apply(Action::ShowWeek);
    assert!(!app.is_typing());
}

/// The keymap is told whether the thread cursor is on screen, so `J`/`K`
/// never move a highlight nobody can see.
///
/// The Today screen draws that cursor on the focused thread panel and
/// nowhere else; the week screen draws it on every row of its list and its
/// own rule offers `J/K a thread`; the narrow layout draws no thread list at
/// all. The bare `typing` flag could not tell those apart, so `j` moved the
/// cursor from all four of Today's focus states.
#[test]
fn the_thread_cursor_is_only_live_where_it_is_drawn() {
    let mut app = app();
    screen(&mut app, 120, 40);
    assert!(app.mode().typing);
    assert!(!app.mode().thread_cursor, "the conversation has focus");

    app.apply(Action::NextPanel);
    app.apply(Action::NextPanel);
    assert_eq!(app.focus(), Focus::Threads);
    assert!(app.mode().thread_cursor, "the thread panel has focus");
    assert!(!app.mode().typing);

    app.apply(Action::NextPanel);
    assert_eq!(app.focus(), Focus::Trickle);
    assert!(!app.mode().thread_cursor, "the trickle has focus");

    // The week draws the cursor whatever has focus — nothing there does.
    app.apply(Action::ShowWeek);
    assert!(app.mode().thread_cursor);
    assert!(!app.mode().typing);

    // The narrow layout has no thread list, and its own rule says so.
    app.apply(Action::ShowToday);
    screen(&mut app, 80, 24);
    assert!(!app.mode().thread_cursor);
    assert!(app.mode().typing);
}

/// Every cell of `buf`, row by row, as the terminal would show it.
fn screen_text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|row| {
            (0..buf.area.width)
                .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let area = buf.area;
    let mut grid = Grid::new(&mut buf, area);
    app.draw(&mut grid);
    screen_text(&buf)
}

/// The width boundary picks a different screen rather than shrinking one.
///
/// Told apart by a marker only one of them draws: the narrow screen's nav
/// abbreviates "The week" to "Week", and only the wide screen has room for
/// the three right-hand panels.
#[test]
fn the_narrow_boundary_switches_screens() {
    let mut app = app();
    for width in [80u16, NARROW_BELOW - 1] {
        let text = screen(&mut app, width, 24);
        assert!(
            text.contains("Today  Week"),
            "the narrow nav is missing at {width}"
        );
        assert!(
            !text.contains("What keeps coming back"),
            "a wide panel survived at {width}"
        );
    }
    for width in [NARROW_BELOW, 120] {
        let text = screen(&mut app, width, 30);
        assert!(
            text.contains("Today   The week"),
            "the wide nav is missing at {width}"
        );
        assert!(
            text.contains("What keeps coming back"),
            "the thread panel is missing at {width}"
        );
    }
}

/// The week screen is drawn at its own layout whatever the width, because it
/// has no narrow variant in the design: the day header is there at 60
/// columns as it is at 120.
#[test]
fn the_week_screen_has_no_narrow_variant() {
    let mut app = app();
    app.apply(Action::ShowWeek);
    for width in [60u16, 80, 120] {
        let text = screen(&mut app, width, 30);
        assert!(
            text.contains("Fri Sat Sun"),
            "the week's day header is missing at {width}"
        );
        assert!(
            !text.contains("Just remembered"),
            "the narrow screen was drawn at {width}"
        );
    }
}
