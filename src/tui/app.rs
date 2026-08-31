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

use chrono::Timelike;

use crate::{
    text,
    tui::{
        grid::Grid,
        model::{Speaker, Turn, Workspace},
        screen::{self, chrome::View, Focus},
    },
};

/// Below this many columns the narrow layout is drawn instead of the wide one.
///
/// The design gives 120x40 and one narrow variant at 80x24, so the boundary is
/// somewhere between. 100 is the midpoint: a terminal at 96 columns has no room
/// for a 48-column aside beside a readable conversation, and one at 104 does.
pub const NARROW_BELOW: u16 = 100;

/// The design's own width, and what an undrawn [`App`] assumes it has.
const DESIGN_COLUMNS: u16 = 120;
/// And its rows, for the same reason.
const DESIGN_ROWS: u16 = 40;

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
    /// Leave. Reached by `Esc` while idle, `q` and `^C`.
    ///
    /// Idle `Esc` quits from anywhere, including mid-draft, and takes the draft
    /// with it — the design gives `Esc` "back" only on the settings and first-run
    /// screens, and from Today and the week there is nothing to go back to. It
    /// is the one key that discards typing, which is the usual terminal
    /// convention; the draft is not persisted anywhere, so leaving is leaving.
    /// In-flight `Esc` is [`Action::Cancel`], not this.
    Quit,
    /// Stop an in-flight turn. Reached by `Esc` while a reply is streaming.
    ///
    /// Does not leave the pane. A second `Esc` after the turn has stopped is
    /// [`Action::Quit`].
    Cancel,
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
    /// How many rows the last draw was given, for the same reason and with the
    /// same default.
    ///
    /// Needed because a short band drops whole panels — the trickle below 33 rows,
    /// the thread list below 20 — so which stops the `Tab` cycle offers depends on
    /// the height as much as on the width.
    pub rows: u16,
    /// Whether the loop should keep going.
    pub running: bool,
    /// Index of the in-flight assistant turn, if a reply is streaming.
    ///
    /// An index rather than "the last turn", because an execute-time notice can
    /// land as its own turn while the reply is still open. `None` is idle.
    in_flight: Option<usize>,
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
            rows: DESIGN_ROWS,
            running: true,
            in_flight: None,
        }
    }

    /// Swap in a freshly rebuilt workspace, keeping the user's place.
    ///
    /// The live path rebuilds the model on every 250 ms tick, and two things
    /// about that rebuild must not disturb the user: the **conversation** (the
    /// draft in particular — the view build always returns it empty, and a
    /// rebuild that replaced it would erase typing four times a second) and
    /// the **day the week screen has open** (the build opens on today, and a
    /// rebuild that reset the selection would fight `H`/`L`). Everything the
    /// graph says — the clock, the week, the logs, the ribbon, the panels —
    /// is taken from the fresh model. The view, the focus, both cursors and
    /// the terminal size are [`App`] fields and survive untouched.
    pub fn refresh(&mut self, workspace: Workspace) {
        let mut fresh = workspace;
        fresh.conversation = std::mem::take(&mut self.workspace.conversation);
        fresh.week.selected = self.workspace.week.selected;
        self.workspace = fresh;
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
            Action::NextPanel => self.focus = self.focus.next_in(&self.panels()),
            Action::PreviousPanel => self.focus = self.focus.previous_in(&self.panels()),
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
            Action::Send => self.send_draft(),
            // The cancellation handle lives on the live path's turn drive;
            // this only refuses to quit. The truncated turn stays until
            // `finish_turn` hears that the stream actually stopped.
            Action::Cancel => {}
            Action::Ignore => {}
        }
    }

    /// Whether a companion turn is currently streaming.
    pub fn turn_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    /// The user text of the in-flight turn.
    ///
    /// Some for the whole flight, not a one-shot: the live path reads this to
    /// spawn `Session::turn` only when Send just opened that flight.
    /// `--demo` never does.
    pub fn outbound(&self) -> Option<&str> {
        let index = self.in_flight?.checked_sub(1)?;
        match self.workspace.conversation.turns.get(index) {
            Some(Turn::Said {
                speaker: Speaker::Person,
                text,
                ..
            }) => Some(text.as_str()),
            _ => None,
        }
    }

    /// Append a streamed token onto the in-flight assistant turn.
    pub fn append_token(&mut self, token: &str) {
        let pending = text::get("tui.turn_pending");
        if let Some(body) = self.in_flight_text_mut() {
            if body == pending {
                body.clear();
            }
            body.push_str(token);
        }
    }

    /// Close the in-flight turn: the completed reply, a cancellation, or a
    /// classified failure rendered as the turn itself.
    pub fn finish_turn(&mut self, outcome: TurnOutcome) {
        match outcome {
            TurnOutcome::Completed(reply) => {
                if let Some(body) = self.in_flight_text_mut() {
                    *body = reply;
                }
            }
            TurnOutcome::Cancelled => {
                let pending = text::get("tui.turn_pending");
                if let Some(body) = self.in_flight_text_mut() {
                    if body.is_empty() || body == pending {
                        *body = text::get("companion.cancelled").to_owned();
                    }
                }
            }
            TurnOutcome::Failed(message) => {
                if let Some(body) = self.in_flight_text_mut() {
                    *body = message;
                }
            }
        }
        self.in_flight = None;
    }

    /// An execute-time diagnostic, drawn as a turn so it is not a print under
    /// the alternate screen.
    pub fn note(&mut self, message: &str) {
        if message.is_empty() {
            return;
        }
        self.workspace.conversation.turns.push(Turn::Said {
            time: stamp(),
            speaker: Speaker::Mooshik,
            text: message.to_owned(),
        });
    }

    /// Move a non-empty draft into a user turn and open a pending assistant.
    ///
    /// Empty (or whitespace-only) drafts are a no-op: Enter must not send a
    /// blank turn. A Send while a turn is already in flight is ignored so the
    /// draft is not consumed by a key that cannot start a second reply.
    fn send_draft(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let draft = self.workspace.conversation.composer.draft.trim();
        if draft.is_empty() {
            return;
        }
        let text = draft.to_owned();
        self.workspace.conversation.composer.draft.clear();
        let time = stamp();
        self.workspace.conversation.turns.push(Turn::Said {
            time: time.clone(),
            speaker: Speaker::Person,
            text,
        });
        self.workspace.conversation.turns.push(Turn::Said {
            time,
            speaker: Speaker::Mooshik,
            text: text::get("tui.turn_pending").to_owned(),
        });
        self.in_flight = Some(self.workspace.conversation.turns.len() - 1);
    }

    fn in_flight_text_mut(&mut self) -> Option<&mut String> {
        let index = self.in_flight?;
        match self.workspace.conversation.turns.get_mut(index) {
            Some(Turn::Said {
                speaker: Speaker::Mooshik,
                text,
                ..
            }) => Some(text),
            _ => None,
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
    /// **Only the wide Today rule advertises `Tab panel`, and this is what makes
    /// that promise true.** The week screen returns nothing at all — `1b` draws
    /// nine panels and gives focus to none of them, its own rule offers `H/L a
    /// day · J/K a thread`, and it never reads [`App::focus`] — so `Tab` there is
    /// a no-op instead of a silent mutation. The narrow layout returns the one
    /// focus its two panels share, so `Tab` moves nothing there either; that is
    /// `1h`'s own shape, and its rule no longer names the key. It did, and the
    /// claim this comment used to make — that the cycle makes the hint true
    /// rather than approximately true — was false on exactly the screen where the
    /// cycle is one element long.
    fn panels(&self) -> Vec<Focus> {
        match self.view {
            View::Week => Vec::new(),
            _ if self.columns < NARROW_BELOW => Focus::NARROW.to_vec(),
            // Everything else the wide screen decides about its own panels —
            // which of them a standing caution replaces, and which of them a
            // short band leaves no rows for. The predicate lives with the screen
            // that makes those decisions, because a cycle and a screen that
            // disagree about which panels exist is the whole bug.
            //
            // A `Vec` rather than a `&'static [Focus]`: the answer now depends on
            // the terminal, so there is no fixed set to point at, and this runs
            // once per keypress rather than once per cell.
            _ => screen::today::focusable(&self.workspace, self.columns, self.rows),
        }
    }

    /// The focus the current screen can actually draw.
    pub fn focus(&self) -> Focus {
        self.focus.within(&self.panels())
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
        self.rows = grid.height();
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

    /// What the keyboard means on the screen that is showing.
    pub fn mode(&self) -> Mode {
        Mode {
            typing: self.is_typing(),
            // The week screen draws the thread cursor on every row of its list
            // and its own rule offers `J/K a thread`; the Today screen draws it
            // only while the thread panel holds focus (see `aside::threads`);
            // the narrow layout draws no thread list at all.
            //
            // No width test: `focus()` clamps through `panels()`, which returns
            // the one-stop narrow cycle below `NARROW_BELOW`, so `Focus::Threads`
            // is already unreachable there. A `columns >= NARROW_BELOW &&` in
            // front of this could never change the answer — the same dead guard
            // round eight took out of `aside::leans_on`.
            thread_cursor: match self.view {
                View::Week => true,
                _ => self.focus() == Focus::Threads,
            },
            in_flight: self.turn_in_flight(),
        }
    }
}

/// What a key means right now — the two things about the screen underneath that
/// the keymap has to know, plus whether a turn is in flight so `Esc` can cancel
/// rather than leave.
///
/// It replaced a bare `typing: bool`, which was not enough to answer the second
/// question: `j` and `k` moved the thread cursor from anywhere, including the
/// three focus states in which nothing on screen draws that cursor. So
/// `hint_today` promised `J/K a thread` in all four and the keys did something
/// invisible in three of them. See [`crate::tui::input::action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mode {
    /// Keystrokes reach the draft, so a letter is a letter.
    pub typing: bool,
    /// The panel that draws the thread cursor holds focus, so `J`/`K` move
    /// something the reader can see.
    pub thread_cursor: bool,
    /// A companion reply is streaming, so `Esc` is cancellation not leave.
    pub in_flight: bool,
}

/// How an in-flight turn ended.
///
/// Strings, not [`crate::companion::CompanionError`]: the screens stay a pure
/// function of the model, and the live path classifies the error (through
/// `Display` / `en.toml`) before it reaches here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    /// The model finished. The full reply replaces any streamed prefix.
    Completed(String),
    /// `Esc` (or the chat loop's Ctrl-C equivalent) stopped the stream.
    /// A truncated reply is left in place; an empty one becomes the cancelled
    /// sentence so the key is not silent.
    Cancelled,
    /// A classified failure, already rendered, becomes the turn.
    Failed(String),
}

/// "14:22" in the locale's own clock template, for a turn stamped now.
pub(crate) fn stamp() -> String {
    let now = chrono::Local::now();
    text::get("tui.clock")
        .replace("{hour}", &format!("{:02}", now.hour()))
        .replace("{minute}", &format!("{:02}", now.minute()))
}

/// Move `at` by `delta`, clamped to `0..=last`.
fn step(at: usize, delta: isize, last: usize) -> usize {
    let moved = isize::try_from(at).unwrap_or(0).saturating_add(delta);
    usize::try_from(moved.max(0)).unwrap_or(0).min(last)
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
