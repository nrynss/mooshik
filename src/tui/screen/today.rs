//! The Today screen — artboards `1a`, `1c` and `1d`.
//!
//! "The default — the pane that stays open in a tmux split all day."
//!
//! All three artboards are this one function. `1c` differs only in that the
//! conversation contains a [`Turn::Recalled`], and `1d` only in that it contains
//! a [`Turn::Cautioned`]; the screen reads the tail of the conversation to
//! decide which key hint to offer and whether the middle panel shows the thread
//! list or what leans on the thing about to change. There is no mode flag,
//! because there is no mode: a caution is a turn, and the user can keep typing
//! straight past it.

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Turn, Workspace},
    },
};

use super::{aside, chrome, conversation, Band, Focus};

/// The column the right-hand column starts at on a 120-column screen, and the
/// proportion that keeps when the terminal is a different width.
const SPLIT_NUMERATOR: u16 = 72;
const SPLIT_DENOMINATOR: u16 = 120;
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
        let left = (width.saturating_mul(SPLIT_NUMERATOR) / SPLIT_DENOMINATOR).max(1);
        let rows = band.rows();
        let trickle_rows = if rows >= TRICKLE_FLOOR {
            TRICKLE_ROWS
        } else {
            0
        };
        let today_rows = TODAY_ROWS.min(rows.saturating_sub(trickle_rows));
        Self {
            left,
            right: width.saturating_sub(left),
            conversation_rows: rows.saturating_sub(COMPOSER_ROWS),
            today_rows,
            threads_rows: rows.saturating_sub(today_rows).saturating_sub(trickle_rows),
            trickle_rows,
        }
    }
}

/// What the tail of the conversation is asking of the user, which decides the
/// middle panel and the key hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tail {
    /// Nothing special — the thread list, and the ordinary key hints.
    Ordinary,
    /// Something came back, so the hints offer to open its source.
    Recalled,
    /// A caution is standing, so the middle panel shows what leans on it.
    Cautioned,
}

impl Tail {
    /// Read the conversation's last turn.
    ///
    /// Only the *last* turn counts: a caution from an hour ago has been answered
    /// and should not still be reshaping the screen, and `1d` shows the caution
    /// as the most recent thing said.
    fn of(workspace: &Workspace) -> Self {
        match workspace.conversation.turns.last() {
            Some(Turn::Cautioned(_)) => Self::Cautioned,
            Some(Turn::Recalled(_)) => Self::Recalled,
            _ => Self::Ordinary,
        }
    }

    /// The key hint this tail offers.
    fn hint(self) -> &'static str {
        match self {
            Self::Ordinary => text::get("tui.hint_today"),
            Self::Recalled => text::get("tui.hint_recall"),
            Self::Cautioned => text::get("tui.hint_caution"),
        }
    }
}

/// Draw the Today screen over the whole of `grid`.
pub fn draw(grid: &mut Grid<'_>, workspace: &Workspace, focus: Focus) {
    let band = Band::new(grid.height(), chrome::Margins::WIDE);
    let split = Split::new(grid.width(), band);
    let tail = Tail::of(workspace);

    let subject = format!(
        "{}{}{}",
        workspace.now.long_date,
        text::get("tui.separator"),
        workspace.now.time
    );
    chrome::title(grid, band.margins, &subject, chrome::View::Today);

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
            COMPOSER_ROWS,
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
        (Tail::Cautioned, Some(thread)) => aside::leans_on(
            grid,
            thread,
            Place::new(split.left, middle_row, split.right, split.threads_rows),
        ),
        _ => aside::threads(
            grid,
            &workspace.threads,
            focus == Focus::Threads,
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
        tail.hint(),
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
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        draw(&mut grid, workspace, focus);
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

    /// A standing caution swaps the middle panel and the key hint — and it is
    /// still the same screen, with the conversation and composer in place.
    #[test]
    fn a_standing_caution_swaps_the_middle_panel() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![Turn::Cautioned(Caution {
            title: "One thing before you do".to_owned(),
            lead: "You've held to this every day.".to_owned(),
            leaning: vec!["The short postmortem".to_owned()],
            because: "Nothing's changed".to_owned(),
        })];
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What leans on this"));
        assert!(!text.contains("What keeps coming back"));
        assert!(text.contains("show what leans on it"));
        // Still the Today screen, not a modal over it.
        assert!(text.contains("The conversation"));
        assert!(text.contains("Just remembered"));
    }

    /// A recall in the tail keeps the thread list but offers to open the source.
    #[test]
    fn a_recall_in_the_tail_changes_only_the_hint() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![Turn::Recalled(Recall {
            source: "From Monday 24 August".to_owned(),
            quote: "Blocking is honest.".to_owned(),
            because: "Every day this week".to_owned(),
        })];
        let text = all_text(&drawn(120, 40, &workspace, Focus::Conversation));
        assert!(text.contains("What keeps coming back"));
        assert!(text.contains("open the source"));
    }

    /// A caution earlier in the conversation has been answered and no longer
    /// reshapes the screen — only the last turn counts.
    #[test]
    fn an_answered_caution_no_longer_reshapes_the_screen() {
        let mut workspace = workspace();
        workspace.conversation.turns = vec![
            Turn::Cautioned(Caution {
                title: "One thing before you do".to_owned(),
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
            title: "One thing".to_owned(),
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
