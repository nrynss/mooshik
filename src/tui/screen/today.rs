//! The Today screen — artboards `1a`, `1c` and `1d`.
//!
//! "The default — the pane that stays open in a tmux split all day."
//!
//! All three artboards are this one function. `1c` differs only in that the
//! conversation contains a [`Turn::Recalled`], and `1d` only in that it contains
//! a [`Turn::Cautioned`]; the screen reads the tail of the conversation to
//! decide whether the middle panel shows the thread list or what leans on the
//! thing about to change. There is no mode flag, because there is no mode: a
//! caution is a turn, and the user can keep typing straight past it.
//!
//! The artboards also change the *key hint* between the three, and this does
//! not. `1c` offers "Enter open the source" and `1d` "Enter show what leans on
//! it", and neither key is bound: `Enter` sends. A hint is a promise, so the one
//! rule the three states share is the one that is drawn, and the state-specific
//! hints come back with the keys that answer them.

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Turn, Workspace},
    },
};

use super::{aside, chrome, conversation, joined, Band, Focus};

/// The column the right-hand column starts at on a 120-column screen, and the
/// proportion that keeps when the terminal is a different width.
///
/// `u32`, because the product is what overflows: `width * 72` passes `u16::MAX`
/// at 911 columns, and a *saturating* multiply there inverted the whole layout
/// — at 2000 columns the conversation got 546 and the aside 1454.
const SPLIT_NUMERATOR: u32 = 72;
const SPLIT_DENOMINATOR: u32 = 120;
/// Rows the composer takes: two rules, the draft, and the reassurance.
const COMPOSER_ROWS: u16 = 4;
/// Rows the Today panel takes.
const TODAY_ROWS: u16 = 16;
/// Rows the trickle takes.
const TRICKLE_ROWS: u16 = 7;
/// Below this many panel rows the trickle is dropped rather than squeezed — the
/// design's own narrow behaviour, applied to a short terminal.
const TRICKLE_FLOOR: u16 = 30;

/// Where the Today screen's panels sit, derived from the grid.
#[derive(Debug, Clone, Copy)]
struct Split {
    left: u16,
    right: u16,
    composer_rows: u16,
    conversation_rows: u16,
    today_rows: u16,
    threads_rows: u16,
    trickle_rows: u16,
}

impl Split {
    /// Divide `width` columns and `band.rows()` rows between the panels.
    ///
    /// The left column keeps the artboard's 72-of-120 proportion at any width,
    /// and the right column's three panels keep their heights until the band is
    /// too short — at which point the trickle is dropped whole rather than
    /// squeezed to a row or two, because a two-line trickle reads as an error.
    fn new(width: u16, band: Band) -> Self {
        let left = u16::try_from(u32::from(width) * SPLIT_NUMERATOR / SPLIT_DENOMINATOR)
            .unwrap_or(u16::MAX)
            .max(1);
        let rows = band.rows();
        // The composer takes what the band can spare rather than its four rows
        // regardless. A band shorter than four gave it four anyway, and its
        // frame, its draft and the bottom rule all landed on the same row — the
        // same "squeezed to nothing beats drawn over something else" call
        // `trickle_rows` makes just below, applied to the panel that was exempt.
        let composer_rows = COMPOSER_ROWS.min(rows);
        let trickle_rows = if rows >= TRICKLE_FLOOR {
            TRICKLE_ROWS
        } else {
            0
        };
        let today_rows = TODAY_ROWS.min(rows.saturating_sub(trickle_rows));
        Self {
            left,
            right: width.saturating_sub(left),
            composer_rows,
            conversation_rows: rows.saturating_sub(composer_rows),
            today_rows,
            threads_rows: rows.saturating_sub(today_rows).saturating_sub(trickle_rows),
            trickle_rows,
        }
    }
}

/// What the tail of the conversation is asking of the user, which decides the
/// middle panel.
///
/// Two states, not three. A recall in the tail used to be its own variant, and
/// the only thing it changed was a key hint naming a key nothing bound — so a
/// recall now reshapes nothing, which is also what the artboards show: `1c` is
/// the ordinary screen with a card in the scroll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tail {
    /// Nothing standing — the thread list.
    Ordinary,
    /// A caution is standing, so the middle panel shows what leans on it.
    Cautioned,
}

impl Tail {
    /// Read the conversation's last turn.
    ///
    /// Only the *last* turn counts: a caution from an hour ago has been answered
    /// and should not still be reshaping the screen.
    ///
    /// **This diverges from `1d`, deliberately.** The artboard draws two further
    /// turns after the caution — "Keep it. I'll widen the ring instead." and
    /// "Noted. Nothing changed." — and still shows "What leans on this" in the
    /// middle panel, so on `1d` the caution is *not* the most recent thing said
    /// and the panel has outlived it. The argument `1d` is making is about the
    /// moment the caution stands, and a panel that keeps standing after the user
    /// has answered is a modal in everything but name. So the screen reads the
    /// tail, and `demo_caution.toml` stops at the caution to draw the artboard;
    /// its own header says the same thing from the fixture's side.
    fn of(workspace: &Workspace) -> Self {
        match workspace.conversation.turns.last() {
            Some(Turn::Cautioned(_)) => Self::Cautioned,
            _ => Self::Ordinary,
        }
    }
}

/// Whether the middle panel is showing what leans on a standing caution rather
/// than the thread list.
///
/// Exported because [`App::panels`](crate::tui::app::App::panels) needs the same
/// answer: `aside::leans_on` is fixed at [`Kind::Caution`] and ignores focus, so
/// a `Tab` cycle that still contained [`Focus::Threads`] under a standing caution
/// put focus on a panel that is not on screen — no accent anywhere, and `J`/`K`
/// moving a cursor nothing draws while the rule promised `J/K a thread`. One
/// predicate rather than the same condition written twice, because the two
/// disagreeing is exactly the bug.
pub fn shows_leans_on(workspace: &Workspace) -> bool {
    Tail::of(workspace) == Tail::Cautioned && !workspace.threads.is_empty()
}

/// Draw the Today screen over the whole of `grid`.
///
/// `thread_cursor` is where `J`/`K` have moved the highlight in the thread list.
/// It is drawn only while that panel holds focus — see [`aside::threads`].
pub fn draw(grid: &mut Grid<'_>, workspace: &Workspace, focus: Focus, thread_cursor: usize) {
    let band = Band::new(grid.height(), chrome::Margins::WIDE);
    let split = Split::new(grid.width(), band);
    let tail = Tail::of(workspace);

    // Joined rather than formatted: the live workspace has no clock yet, and an
    // unconditional separator drew "Mooshik  ·    ·  " on the primary path. See
    // `screen::joined`.
    let subject = joined(
        &[&workspace.now.long_date, &workspace.now.time],
        text::get("tui.separator"),
    );
    if band.title {
        chrome::title(grid, band.margins, &subject, chrome::View::Today);
    }

    conversation::panel(
        grid,
        &workspace.person,
        &workspace.conversation,
        focus == Focus::Conversation,
        Place::new(0, band.top, split.left, split.conversation_rows),
    );
    conversation::composer(
        grid,
        &workspace.conversation,
        focus == Focus::Conversation,
        Place::new(
            0,
            band.top.saturating_add(split.conversation_rows),
            split.left,
            split.composer_rows,
        ),
    );

    aside::today(
        grid,
        &workspace.today,
        &workspace.week.days,
        today_index(workspace),
        focus == Focus::Today,
        Place::new(split.left, band.top, split.right, split.today_rows),
    );

    let middle_row = band.top.saturating_add(split.today_rows);
    // A standing caution replaces the thread list with what leans on the thing
    // about to change. It needs a thread to describe, and the thread being
    // contradicted is the one at the top of the list — the design's caution is
    // always about the strongest thing the user keeps returning to.
    match (tail, workspace.threads.first()) {
        // The same condition [`shows_leans_on`] reports, spelled once here as the
        // match that needs the thread itself.
        (Tail::Cautioned, Some(thread)) => aside::leans_on(
            grid,
            thread,
            Place::new(split.left, middle_row, split.right, split.threads_rows),
        ),
        _ => aside::threads(
            grid,
            &workspace.threads,
            focus == Focus::Threads,
            thread_cursor,
            Place::new(split.left, middle_row, split.right, split.threads_rows),
        ),
    }

    if split.trickle_rows > 0 {
        aside::trickle(
            grid,
            &workspace.trickle,
            focus == Focus::Trickle,
            Place::new(
                split.left,
                middle_row.saturating_add(split.threads_rows),
                split.right,
                split.trickle_rows,
            ),
        );
    }

    chrome::health_rule(
        grid,
        band.margins,
        band.status,
        &workspace.health,
        &workspace.health.scope,
        text::get("tui.hint_today"),
    );
}

/// Which day of the week is today, as an index into `workspace.week.days`.
///
/// Matched on the day-of-month label rather than assumed to be last: the week is
/// Friday-first in the design, so "today" is the seventh column only until
/// somebody looks at a week that has already ended. Falling back to the last day
/// keeps the ribbon drawn rather than leaving it unmarked.
fn today_index(workspace: &Workspace) -> usize {
    workspace
        .week
        .days
        .iter()
        .position(|day| day.day_of_month == workspace.today.day_of_month)
        .unwrap_or_else(|| workspace.week.days.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect};

    use crate::tui::model::{Caution, Day, Recall, Speaker, Stamp, Thread, Turn};

    fn workspace() -> Workspace {
        Workspace {
            person: "Neom".to_owned(),
            now: Stamp {
                long_date: "Thursday 27 August".to_owned(),
                short_date: "Thu 27 Aug".to_owned(),
                time: "14:22".to_owned(),
            },
            today: Day {
                day_of_month: "27".to_owned(),
                ..Day::default()
            },
            threads: vec![Thread {
                summary: "The 512 cap".to_owned(),
                days: [true; 7],
                leaned_on: vec!["The short postmortem".to_owned()],
                ..Thread::default()
            }],
            ..Workspace::default()
        }
    }

    fn drawn(w: u16, h: u16, workspace: &Workspace, focus: Focus) -> Buffer {
        drawn_with_cursor(w, h, workspace, focus, 0)
    }

    fn drawn_with_cursor(
        w: u16,
        h: u16,
        workspace: &Workspace,
        focus: Focus,
        cursor: usize,
    ) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        draw(&mut grid, workspace, focus, cursor);
        buf
    }

    fn all_text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The artboard's own geometry: the right column starts at column 72 of 120.
    #[test]
    fn the_split_matches_the_artboard_at_120_columns() {
        let band = Band::new(40, chrome::Margins::WIDE);
        let split = Split::new(120, band);
        assert_eq!(split.left, 72);
        assert_eq!(split.right, 48);
        assert_eq!(band.top, 1);
        assert_eq!(band.status, 39);
        // 37 panel rows, divided as the artboard divides them.
        assert_eq!(band.rows(), 37);
        assert_eq!(split.today_rows, 16);
        assert_eq!(split.threads_rows, 14);
        assert_eq!(split.trickle_rows, 7);
        assert_eq!(split.conversation_rows, 33);
    }

    /// The proportion holds at other widths rather than the split staying at a
    /// fixed 72 and swallowing a narrow terminal.
    #[test]
    fn the_split_keeps_its_proportion_at_other_widths() {
        for width in [80u16, 100, 160, 200] {
            let split = Split::new(width, Band::new(40, chrome::Margins::WIDE));
            assert!(
                split.left > 0 && split.right > 0,
                "a column vanished at {width}"
            );
            assert_eq!(split.left + split.right, width);
            assert!(
                split.left > split.right,
                "the split inverted at {width}: {} left, {} right",
                split.left,
                split.right
            );
        }
        // Past 910 columns `width * 72` leaves `u16`. It used to saturate, and
        // the split inverted: 546 for the conversation, 1454 for the aside.
        for width in [911u16, 1200, 2000, u16::MAX] {
            let split = Split::new(width, Band::new(40, chrome::Margins::WIDE));
            assert_eq!(split.left + split.right, width);
            assert_eq!(
                split.left,
                u16::try_from(u32::from(width) * 72 / 120).unwrap(),
                "the proportion broke at {width}"
            );
        }
    }

    /// A short terminal drops the trickle whole rather than squeezing it to a
    /// row or two, which would read as a rendering error.
    #[test]
    fn a_short_band_drops_the_trickle_whole() {
        let split = Split::new(120, Band::new(24, chrome::Margins::WIDE));
        assert_eq!(split.trickle_rows, 0);
        assert!(split.today_rows > 0);
        let split = Split::new(120, Band::new(40, chrome::Margins::WIDE));
        assert_eq!(split.trickle_rows, TRICKLE_ROWS);
    }

    /// A band too short for a thread panel gives it zero rows, and a panel with no
    /// interior draws no name — not a bare ` What keeps coming back ` floating on
    /// the blank row above the bottom rule with no frame around it.
    ///
    /// Reachable at any terminal 18 rows or shorter and 100 columns or wider,
    /// which is a tmux pane, not a pathological size.
    #[test]
    fn a_band_with_no_room_for_the_thread_panel_draws_no_title() {
        let split = Split::new(120, Band::new(18, chrome::Margins::WIDE));
        assert_eq!(split.threads_rows, 0, "the band is not short enough");
        assert_eq!(split.trickle_rows, 0);

        let workspace = workspace();
        let text = all_text(&drawn(120, 18, &workspace, Focus::Conversation));
        assert!(
            !text.contains("What keeps coming back"),
            "a frameless title was drawn: {text}"
        );
        // The screen is otherwise whole: the Today panel and both rules.
        assert!(text.contains("Today"), "{text}");
        assert!(text.contains("Tab panel"), "{text}");

        // Three rows of frame is the least that has an interior, so at 22 rows —
        // a 19-row band, 16 of them the Today panel's — its name comes back.
        assert_eq!(
            Split::new(120, Band::new(22, chrome::Margins::WIDE)).threads_rows,
            3
        );
        let text = all_text(&drawn(120, 22, &workspace, Focus::Conversation));
        assert!(text.contains("What keeps coming back"), "{text}");
    }

    /// The whole screen draws without panicking at any size, down to one cell.
    #[test]
    fn every_size_draws_without_panicking() {
        let workspace = workspace();
        for (w, h) in [
            (1u16, 1u16),
            (10, 3),
            (40, 12),
            (80, 24),
            (120, 40),
            (200, 60),
        ] {
            let buf = drawn(w, h, &workspace, Focus::Conversation);
            assert_eq!(buf.area.width, w);
            assert_eq!(buf.area.height, h);
        }
    }

    /// The ordinary screen shows the thread list and the ordinary key hints.
    #[test]
    fn the_ordinary_tail_shows_the_thread_list() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![Turn::Said {
            time: "09:04".to_owned(),
            speaker: Speaker::Person,
            text: "Postmortem's done.".to_owned(),
        }];
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What keeps coming back"));
        assert!(!text.contains("What leans on this"));
        assert!(text.contains("Tab panel"));
    }

    /// A standing caution swaps the middle panel — and it is still the same
    /// screen, with the conversation and composer in place.
    #[test]
    fn a_standing_caution_swaps_the_middle_panel() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![Turn::Cautioned(Caution {
            lead: "You've held to this every day.".to_owned(),
            leaning: vec!["The short postmortem".to_owned()],
            because: "Nothing's changed".to_owned(),
        })];
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What leans on this"));
        assert!(!text.contains("What keeps coming back"));
        // Still the Today screen, not a modal over it.
        assert!(text.contains("The conversation"));
        assert!(text.contains("Just remembered"));
    }

    /// A recall in the tail reshapes nothing: it is a card in the scroll, and
    /// the screen around it is the ordinary one.
    #[test]
    fn a_recall_in_the_tail_reshapes_nothing() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![Turn::Recalled(Recall {
            source: "From Monday 24 August".to_owned(),
            quote: "Blocking is honest.".to_owned(),
            because: "Every day this week".to_owned(),
        })];
        assert_eq!(Tail::of(&workspace), Tail::Ordinary);
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What keeps coming back"));
        assert!(
            text.contains("From Monday 24 August"),
            "the card is missing"
        );
        assert!(text.contains("Tab panel"), "the ordinary hint is missing");
    }

    /// A caution earlier in the conversation has been answered and no longer
    /// reshapes the screen — only the last turn counts.
    #[test]
    fn an_answered_caution_no_longer_reshapes_the_screen() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![
            Turn::Cautioned(Caution {
                lead: "You've held to this.".to_owned(),
                leaning: vec!["The short postmortem".to_owned()],
                because: "Nothing's changed".to_owned(),
            }),
            Turn::Said {
                time: "15:05".to_owned(),
                speaker: Speaker::Person,
                text: "Keep it. I'll widen the ring instead.".to_owned(),
            },
        ];
        assert_eq!(Tail::of(&workspace), Tail::Ordinary);
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What keeps coming back"));
    }

    /// A caution with no threads behind it falls back to the list rather than
    /// drawing an empty caution panel.
    #[test]
    fn a_caution_without_a_thread_falls_back_to_the_list() {
        let mut workspace = workspace();
        workspace.threads.clear();
        workspace.conversation.turns = vec![Turn::Cautioned(Caution {
            lead: "x".to_owned(),
            leaning: Vec::new(),
            because: "y".to_owned(),
        })];
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What keeps coming back"));
        assert!(!text.contains("What leans on this"));
    }

    /// Today is found in the week by its date, and a week that does not contain
    /// it still draws a marked ribbon rather than none.
    #[test]
    fn today_is_located_in_the_week_by_its_date() {
        let mut workspace = workspace();
        workspace.week.days = ["25", "26", "27"]
            .into_iter()
            .map(|d| Day {
                day_of_month: d.to_owned(),
                ..Day::default()
            })
            .collect();
        assert_eq!(today_index(&workspace), 2);

        workspace.today.day_of_month = "31".to_owned();
        assert_eq!(
            today_index(&workspace),
            2,
            "an absent today falls back to the last day"
        );

        workspace.week.days.clear();
        assert_eq!(
            today_index(&workspace),
            0,
            "an empty week does not underflow"
        );
    }

    /// The thread cursor is drawn on this screen, and only while the panel it
    /// belongs to holds focus — otherwise a bright row would claim a cursor the
    /// keys are not driving.
    #[test]
    fn the_thread_cursor_shows_only_on_the_focused_panel() {
        use crate::tui::theme::{Role, Strength};

        let mut workspace = workspace();
        workspace.threads = (0..3)
            .map(|n| Thread {
                summary: format!("Thought {n}"),
                days: [true; 7],
                ..Thread::default()
            })
            .collect();

        // The thread panel's interior starts at column 73, row 18; text sits at
        // `aside`'s own thread column past the marks.
        let text_col = 73 + aside::THREAD_TEXT;
        let second = 19;

        let focused = drawn_with_cursor(120, 40, &workspace, Focus::Threads, 1);
        let cell = &focused[(text_col, second)];
        assert_eq!(cell.fg, Role::Strongest.color(), "the cursor is not drawn");

        // Unfocused, row 1 keeps its ranking colour — the second step.
        let idle = drawn_with_cursor(120, 40, &workspace, Focus::Today, 1);
        let cell = &idle[(text_col, second)];
        assert_eq!(cell.fg, Strength::from_rank(1).role().color());
    }

    /// Focus accents exactly the panel that holds it, and no other.
    ///
    /// Checked at each panel's own top-left corner: `1i` gives the accent to
    /// "the focused panel", so two accented frames at once would mean two
    /// answers to "where am I".
    #[test]
    fn focus_accents_exactly_one_panel() {
        use crate::tui::theme::Role;

        // (focus, the corner of the panel that should be accented). The
        // conversation and its composer share focus — the composer is where the
        // conversation is typed, not a fifth panel — so both corners light.
        let corners: [(Focus, &[(u16, u16)]); 4] = [
            (Focus::Conversation, &[(0, 1), (0, 34)]),
            (Focus::Today, &[(72, 1)]),
            (Focus::Threads, &[(72, 17)]),
            (Focus::Trickle, &[(72, 31)]),
        ];
        let all: Vec<(u16, u16)> = corners
            .iter()
            .flat_map(|(_, c)| c.iter().copied())
            .collect();

        let workspace = workspace();
        for (focus, lit) in corners {
            let buf = drawn(120, 40, &workspace, focus);
            for corner in &all {
                let accented = buf[*corner].fg == Role::Accent.color();
                assert_eq!(
                    accented,
                    lit.contains(corner),
                    "corner {corner:?} is {} for {focus:?}",
                    if accented { "lit" } else { "dark" }
                );
            }
        }
    }
}
