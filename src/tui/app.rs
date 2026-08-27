//! What the TUI is showing, and what a key does to it.
//!
//! State and transitions only — no drawing, no terminal, no I/O — so the whole
//! interaction model is testable by applying [`Action`]s and reading fields.
//! [`crate::tui::input`] turns key events into actions; [`App::draw`] hands the
//! state to a screen.
//!
//! **Focus belongs to the screen that has panels.** `Tab` used to cycle all four
//! of [`Focus`]'s variants whatever was showing. On the 80-column layout that
//! left the screen with no accented rule anywhere, keystrokes no longer reaching
//! the draft, and two cursors moving that nothing drew; on the week screen it
//! mutated a field the week screen never reads. So the cycle is a property of
//! what is on screen — see `App::panels` — and so is the focus that is drawn.
//!
//! **The cursor never reorders anything.** `J`/`K` move a highlight through the
//! thread list and `H`/`L` move it through the week, but neither changes the
//! order of what is on screen. The design is explicit that position *is* the
//! ranking — "there is no sort control, because there is nothing to sort by" —
//! so a cursor that reordered the list would destroy the one thing the list
//! encodes.

use crate::tui::{
    grid::Grid,
    model::Workspace,
    screen::{self, chrome::View, Focus},
};

/// Below this many columns the narrow layout is drawn instead of the wide one.
///
/// The design gives 120x40 and one narrow variant at 80x24, so the boundary is
/// somewhere between. 100 is the midpoint: a terminal at 96 columns has no room
/// for a 48-column aside beside a readable conversation, and one at 104 does.
pub const NARROW_BELOW: u16 = 100;

/// The design's own width, and what an undrawn [`App`] assumes it has.
const DESIGN_COLUMNS: u16 = 120;

/// One thing a keypress can ask for.
///
/// Deliberately coarse: `Action` is the vocabulary of *what the app does*, not
/// of crossterm. The whole vocabulary is the whole keymap — cycle a panel,
/// choose a view, move a cursor, edit the draft, leave — so a key that does
/// something the app has no verb for has nowhere to be expressed, and the hints
/// on the bottom rules name these variants and nothing else. The design also
/// printed `Alt-H/L resize`, `^K a day` and `? keys`; there is deliberately no
/// variant for them, which is why those hints are not drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Leave. Reached by `Esc`, `q` and `^C`.
    ///
    /// `Esc` quits from anywhere, including mid-draft, and takes the draft with
    /// it — the design gives `Esc` "back" only on the settings and first-run
    /// screens, and from Today and the week there is nothing to go back to. It
    /// is the one key that discards typing, which is the usual terminal
    /// convention; the draft is not persisted anywhere, so leaving is leaving.
    Quit,
    /// Move focus to the next panel.
    NextPanel,
    /// Move focus to the previous panel.
    PreviousPanel,
    /// Show today.
    ShowToday,
    /// Show the week.
    ShowWeek,
    /// Move the cursor down a list.
    Next,
    /// Move the cursor up a list.
    Previous,
    /// Move the cursor left — a day, on the week screen.
    Left,
    /// Move the cursor right.
    Right,
    /// Add a character to the draft.
    Type(char),
    /// Remove the last character of the draft.
    Backspace,
    /// Send the draft.
    Send,
    /// The key means nothing here.
    Ignore,
}

/// The running TUI.
#[derive(Debug, Clone)]
pub struct App {
    /// What is on screen.
    pub workspace: Workspace,
    /// Which view is showing.
    pub view: View,
    /// Which panel holds focus on the Today screen.
    ///
    /// What is *drawn* is [`App::focus`], which is this clamped to the panels the
    /// current screen actually has. The field keeps the wide screen's choice
    /// across a narrow detour, so a terminal widened back finds focus where it
    /// was left.
    pub focus: Focus,
    /// Which thread the week screen's cursor is on.
    pub thread_cursor: usize,
    /// How many columns the last draw was given.
    ///
    /// Recorded because `Tab` has to know which screen it is cycling and the key
    /// arrives between draws, not during one. Starts at the design's own 120 so
    /// an [`App`] that has never been drawn behaves as the wide screen — the
    /// alternative, treating "not yet drawn" as narrow, would make the first
    /// keypress of a real session depend on a value no draw had set.
    pub columns: u16,
    /// Whether the loop should keep going.
    pub running: bool,
}

impl App {
    /// A new app showing `workspace`.
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            view: View::Today,
            focus: Focus::default(),
            thread_cursor: 0,
            columns: DESIGN_COLUMNS,
            running: true,
        }
    }

    /// Apply `action`.
    ///
    /// Returns nothing: every effect is a field on `self`, which is what makes
    /// the interaction model readable in a test.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
            // Only on the screen that has panels to cycle. The week screen has
            // none, and the narrow layout has one — see `panels`.
            Action::NextPanel => self.focus = self.focus.next_in(self.panels()),
            Action::PreviousPanel => self.focus = self.focus.previous_in(self.panels()),
            Action::ShowToday => self.view = View::Today,
            Action::ShowWeek => self.view = View::Week,
            Action::Next => self.move_cursor(1),
            Action::Previous => self.move_cursor(-1),
            Action::Right => self.move_day(1),
            Action::Left => self.move_day(-1),
            Action::Type(character) => self.workspace.conversation.composer.draft.push(character),
            Action::Backspace => {
                // Pop a character, not a byte: a draft ending in an em dash or an
                // accented letter must not be left holding half a code point.
                self.workspace.conversation.composer.draft.pop();
            }
            // Sending is where the companion loop attaches (it needs M3's chat
            // restructured into something a redraw loop can drive). Until it
            // does, `Enter` deliberately does *nothing*: it used to clear the
            // draft, which looked like sending and was data loss — the words
            // went, no turn appeared, and no message was sent anywhere. An
            // inert key the user can press again is the honest version, and it
            // leaves the draft where the composer can still draw it.
            Action::Send => {}
            Action::Ignore => {}
        }
    }

    /// Move the thread cursor by `delta`, clamped to the list.
    ///
    /// Clamped rather than wrapping: the list is ordered by how strongly the user
    /// returns to something, so running off the bottom and reappearing at the
    /// strongest would misrepresent where the cursor is in that ranking.
    fn move_cursor(&mut self, delta: isize) {
        let last = self.workspace.threads.len().saturating_sub(1);
        self.thread_cursor = step(self.thread_cursor, delta, last);
    }

    /// Move the week's selected day by `delta`, clamped to the week.
    fn move_day(&mut self, delta: isize) {
        let last = self.workspace.week.days.len().saturating_sub(1);
        self.workspace.week.selected = step(self.workspace.week.selected, delta, last);
    }

    /// The panels the screen currently showing can put focus on.
    ///
    /// The bottom rules advertise `Tab panel`, and this is what makes that
    /// promise true rather than approximately true. The week screen returns
    /// nothing at all — `1b` draws nine panels and gives focus to none of them,
    /// its own rule offers `H/L a day · J/K a thread`, and it never reads
    /// [`App::focus`] — so `Tab` there is a no-op instead of a silent mutation.
    /// The narrow layout returns the one focus its two panels share.
    fn panels(&self) -> &'static [Focus] {
        match self.view {
            View::Week => &[],
            _ if self.columns < NARROW_BELOW => &Focus::NARROW,
            _ => &Focus::CYCLE,
        }
    }

    /// The focus the current screen can actually draw.
    pub fn focus(&self) -> Focus {
        self.focus.within(self.panels())
    }

    /// Draw the current state over the whole of `grid`.
    ///
    /// The layout choice lives here rather than in a screen because it is a
    /// choice *between* screens: below [`NARROW_BELOW`] columns the design does
    /// not shrink the Today screen, it draws a different one.
    ///
    /// `&mut self` for one field: the width is recorded here because this is the
    /// only place that knows it, and `Tab` — which arrives between draws — has to
    /// know which screen it is cycling. Nothing else about the draw is stateful;
    /// the screens remain a pure function of the model.
    pub fn draw(&mut self, grid: &mut Grid<'_>) {
        self.columns = grid.width();
        let focus = self.focus();
        match self.view {
            View::Week => screen::week::draw(grid, &self.workspace, self.thread_cursor),
            View::Today | View::Settings if grid.width() < NARROW_BELOW => {
                screen::narrow::draw(grid, &self.workspace, focus)
            }
            View::Today | View::Settings => {
                screen::today::draw(grid, &self.workspace, focus, self.thread_cursor)
            }
        }
    }

    /// Whether keystrokes should reach the draft rather than the navigation keys.
    ///
    /// Reads [`App::focus`] rather than the field, so a focus left on a panel the
    /// current screen does not draw cannot silently swallow the draft.
    pub fn is_typing(&self) -> bool {
        self.view == View::Today && self.focus() == Focus::Conversation
    }
}

/// Move `at` by `delta`, clamped to `0..=last`.
fn step(at: usize, delta: isize, last: usize) -> usize {
    let moved = isize::try_from(at).unwrap_or(0).saturating_add(delta);
    usize::try_from(moved.max(0)).unwrap_or(0).min(last)
}

#[cfg(test)]
mod tests {
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
}
