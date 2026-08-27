//! The same day at 80x24 — artboard `1h`.
//!
//! The design says exactly what survives and what goes: "What survives: the
//! conversation, one line of what keeps coming back, one line of the trickle,
//! the quiet mark. What goes: the day list and the week ribbon (`^2` shows them
//! full-screen). Nothing scrolls sideways."
//!
//! So this is not a reflow of [`today`](super::today) — it is a different set of
//! decisions about the same data. The three right-hand panels do not become
//! narrower; two of them collapse to a single line each and the third is gone,
//! reachable in full on its own screen. A panel squeezed to eleven columns would
//! technically fit and tell the reader nothing.
//!
//! The two collapsed lines are summaries, and both are built here rather than
//! carried in the model: a thread's own sentence and the trickle's own entries
//! are what they are, and how much of them fits on one 80-column row is a
//! property of this layout.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Thread, Trickle, Workspace},
        theme::Role,
        widget::marks,
        wrap::wrap,
    },
};

use super::{chrome, conversation, Focus};

/// Rows the composer takes here: two rules and the draft, with no room for the
/// reassurance line the wide layout carries.
const COMPOSER_ROWS: u16 = 3;
/// Rows kept below the panels: the thread line, the trickle line, a blank, and
/// the bottom rule.
const TAIL_ROWS: u16 = 4;
/// Column the collapsed thread line's text starts at: seven marks and a space.
const THREAD_TEXT: u16 = marks::WEEK as u16 + 2;

/// Draw the narrow screen over the whole of `grid`.
pub fn draw(grid: &mut Grid<'_>, workspace: &Workspace, focus: Focus) {
    let margins = chrome::Margins::NARROW;
    let width = grid.width();
    let height = grid.height();

    let subject = format!(
        "{}{}{}",
        workspace.now.short_date,
        text::get("tui.separator"),
        workspace.now.time
    );
    grid.run(
        margins.left,
        0,
        [
            Span::styled(text::get("tui.brand"), Role::Strongest.style()),
            Span::styled(
                format!("{}{subject}", text::get("tui.separator")),
                Role::Furniture.style(),
            ),
        ],
    );
    nav(grid, margins);

    let status_row = height.saturating_sub(1);
    let trickle_row = height.saturating_sub(3);
    let thread_row = height.saturating_sub(4);
    // Panels take everything above the tail. A terminal too short for a tail at
    // all gives the panels what is left and drops the collapsed lines, rather
    // than drawing them over the conversation.
    let panel_rows = height.saturating_sub(1).saturating_sub(TAIL_ROWS);

    let conversation_rows = panel_rows.saturating_sub(COMPOSER_ROWS);
    conversation::panel(
        grid,
        &workspace.person,
        &workspace.conversation,
        focus == Focus::Conversation,
        Place::new(0, 1, width, conversation_rows),
    );
    conversation::composer(
        grid,
        &workspace.conversation,
        focus == Focus::Conversation,
        Place::new(0, 1 + conversation_rows, width, COMPOSER_ROWS),
    );

    if panel_rows > COMPOSER_ROWS {
        thread_line(grid, workspace.threads.as_slice(), margins, thread_row);
        trickle_line(grid, &workspace.trickle, margins, trickle_row);
    }

    chrome::health_rule(
        grid,
        margins,
        status_row,
        &workspace.health,
        &workspace.health.short_scope,
        text::get("tui.hint_narrow"),
    );
}

/// The abbreviated nav: `Today  Week  ?`.
fn nav(grid: &mut Grid<'_>, margins: chrome::Margins) {
    let gap = text::get("tui.narrow.nav_gap");
    let items = [
        (text::get("tui.nav_today"), Role::Accent),
        (text::get("tui.narrow.nav_week"), Role::Furniture),
        (text::get("tui.narrow.nav_keys"), Role::Furniture),
    ];
    let width: usize = items.iter().map(|(l, _)| l.chars().count()).sum::<usize>()
        + gap.chars().count() * (items.len() - 1);
    let start = margins
        .right_edge(grid.width())
        .saturating_sub(u16::try_from(width).unwrap_or(u16::MAX));

    let mut spans = Vec::with_capacity(items.len() * 2);
    for (index, (label, role)) in items.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(gap, Role::Furniture.style()));
        }
        spans.push(Span::styled(label, role.style()));
    }
    grid.run(start, 0, spans);
}

/// The strongest thread, on one row, with the count of the rest offered as a key.
///
/// The marks stay: they are seven cells and they carry the whole "every day this
/// week" claim without a word, which is exactly what a single row needs.
fn thread_line(grid: &mut Grid<'_>, threads: &[Thread], margins: chrome::Margins, row: u16) {
    let Some(thread) = threads.first() else {
        return;
    };

    let more = threads.len().saturating_sub(1);
    let hint = if more > 0 {
        text::get("tui.hint_narrow_more").replace("{count}", &more.to_string())
    } else {
        String::new()
    };
    let hint_width = u16::try_from(hint.chars().count()).unwrap_or(0);
    let right = margins.right_edge(grid.width());

    grid.run(margins.left, row, marks::compact(thread.days));
    // The thread's sentence and its reason, on one row, cut to what is left
    // after the marks and the hint.
    let text_col = margins.left.saturating_add(THREAD_TEXT);
    let available = right
        .saturating_sub(text_col)
        .saturating_sub(hint_width.saturating_add(2));
    let joined = if thread.because.is_empty() {
        thread.summary.clone()
    } else {
        format!(
            "{}{}{}",
            thread.summary,
            text::get("tui.trickle_bullet"),
            thread.because.text
        )
    };
    // An ellipsis when the sentence was cut, so a row ending "…overflow blocks,
    // never" reads as truncation rather than as an unfinished thought.
    let mut lines = wrap(&joined, available.saturating_sub(1)).into_iter();
    if let Some(line) = lines.next() {
        let clipped = lines.next().is_some();
        let shown = if clipped { format!("{line}…") } else { line };
        grid.put(text_col, row, &shown, Role::Body.style());
    }
    if !hint.is_empty() {
        grid.put_ending_at(right, row, &hint, Role::Furniture.style());
    }
}

/// The trickle, joined onto one row: `· Just remembered: a, b, c`.
fn trickle_line(grid: &mut Grid<'_>, trickle: &[Trickle], margins: chrome::Margins, row: u16) {
    if trickle.is_empty() {
        return;
    }
    let available = margins
        .right_edge(grid.width())
        .saturating_sub(margins.left);
    let prefix = text::get("tui.narrow_trickle_prefix");

    // Entries are added only while the whole of one still fits, rather than
    // wrapping the joined string and keeping its first line: that cut mid-list
    // and left the row ending on a dangling comma. The full list is a keypress
    // away, so dropping the tail is honest; advertising it with a comma is not.
    let mut line = String::from(prefix);
    let mut width = u16::try_from(prefix.chars().count()).unwrap_or(u16::MAX);
    for (index, item) in trickle.iter().enumerate() {
        let separator = if index == 0 { "" } else { ", " };
        let addition = format!("{separator}{}", item.text);
        let addition_width = u16::try_from(addition.chars().count()).unwrap_or(u16::MAX);
        if width.saturating_add(addition_width) > available {
            break;
        }
        line.push_str(&addition);
        width = width.saturating_add(addition_width);
    }
    // Nothing but the prefix means not even the freshest entry fits, and a bare
    // "· Just remembered:" says less than an empty row.
    if width > u16::try_from(prefix.chars().count()).unwrap_or(u16::MAX) {
        grid.put(margins.left, row, &line, Role::Furniture.style());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect};

    use crate::tui::model::{Health, Justification, Speaker, Stamp, Turn};

    fn workspace() -> Workspace {
        Workspace {
            person: "Neom".to_owned(),
            now: Stamp {
                long_date: "Thursday 27 August".to_owned(),
                short_date: "Thu 27 Aug".to_owned(),
                time: "14:22".to_owned(),
            },
            threads: (0..5)
                .map(|n| Thread {
                    summary: format!("Thought {n}"),
                    days: [true; 7],
                    because: Justification::history("every day this week"),
                    leaned_on: Vec::new(),
                })
                .collect(),
            trickle: vec![
                Trickle::new("12km before standup"),
                Trickle::new("the novel"),
                Trickle::new("call Mum back"),
            ],
            conversation: crate::tui::model::Conversation {
                earlier: Some("... earlier today".to_owned()),
                turns: vec![Turn::Said {
                    time: "14:20".to_owned(),
                    speaker: Speaker::Person,
                    text: "Right. And I still need to call Mum back.".to_owned(),
                }],
                composer: crate::tui::model::Composer {
                    draft: "Called Mum. No answer.".to_owned(),
                },
            },
            health: Health {
                state: "Keeping up".to_owned(),
                scope: "214 things remembered, back to 21 August".to_owned(),
                short_scope: "214 remembered".to_owned(),
                well: true,
            },
            ..Workspace::default()
        }
    }

    fn drawn(w: u16, h: u16, workspace: &Workspace) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        draw(&mut grid, workspace, Focus::Conversation);
        buf
    }

    fn row_text(buf: &Buffer, row: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, row)].symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    fn all_text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|r| row_text(buf, r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The artboard's geometry: the conversation to row 16, the composer to 19,
    /// then the two collapsed lines, a blank, and the bottom rule.
    #[test]
    fn the_geometry_matches_the_artboard() {
        let buf = drawn(80, 24, &workspace());
        assert_eq!(
            buf[(0, 1)].symbol(),
            "┌",
            "the conversation panel starts on row 1"
        );
        assert_eq!(
            buf[(0, 16)].symbol(),
            "└",
            "the conversation closes on row 16"
        );
        assert_eq!(buf[(0, 17)].symbol(), "┌", "the composer starts on row 17");
        assert_eq!(buf[(0, 19)].symbol(), "└", "the composer closes on row 19");
        assert!(row_text(&buf, 22).trim().is_empty(), "row 22 is the blank");
        assert!(row_text(&buf, 23).contains("Keeping up"));
    }

    /// The two collapsed lines are there, and the panels they came from are not.
    #[test]
    fn the_side_panels_collapse_to_one_line_each() {
        let buf = drawn(80, 24, &workspace());
        let text = all_text(&buf);
        assert!(!text.contains("What keeps coming back"), "a panel survived");
        assert!(
            !text.contains("┌─ Just remembered"),
            "the trickle panel survived"
        );
        assert!(
            text.contains("Thought 0"),
            "the strongest thread is missing"
        );
        assert!(
            text.contains("· Just remembered: "),
            "the collapsed trickle line is missing"
        );
        assert!(text.contains("12km before standup"));
    }

    /// The thread line keeps its day marks — seven cells that carry the whole
    /// claim — and offers the rest of the list as a keypress.
    #[test]
    fn the_thread_line_keeps_its_marks_and_counts_the_rest() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 20);
        assert!(row.starts_with(" ▄▄▄▄▄▄▄"), "{row:?}");
        assert!(row.contains("4 more"), "{row:?}");
    }

    /// A clipped thread line says so, rather than ending on a word that reads as
    /// an unfinished thought.
    #[test]
    fn a_clipped_thread_line_is_marked_as_clipped() {
        let mut long = workspace();
        long.threads[0].summary =
            "The ring holds 512 in flight; overflow blocks, never drops".to_owned();
        let buf = drawn(80, 24, &long);
        let row = row_text(&buf, 20);
        assert!(row.contains('…'), "the clip is not marked: {row:?}");

        // A thread that fits is not marked.
        let mut short = workspace();
        short.threads.truncate(1);
        short.threads[0].summary = "Short".to_owned();
        short.threads[0].because = Justification::history("once");
        let buf = drawn(80, 24, &short);
        let row = row_text(&buf, 20);
        assert!(!row.contains('…'), "a line that fits was marked: {row:?}");
    }

    /// A single thread offers no "more" hint, because there is no more.
    #[test]
    fn one_thread_offers_no_more_hint() {
        let mut workspace = workspace();
        workspace.threads.truncate(1);
        let buf = drawn(80, 24, &workspace);
        assert!(!row_text(&buf, 20).contains("more"));
    }

    /// The collapsed trickle never ends on a dangling separator, at any width.
    #[test]
    fn the_trickle_line_never_ends_on_a_comma() {
        for width in [40u16, 50, 60, 70, 80, 99] {
            let buf = drawn(width, 24, &workspace());
            let row = row_text(&buf, 21).trim_end().to_owned();
            assert!(!row.ends_with(','), "dangling comma at {width}: {row:?}");
            assert!(
                !row.trim_end().ends_with(':'),
                "a bare prefix was drawn at {width}: {row:?}"
            );
        }
    }

    /// A row too narrow for even the freshest entry draws nothing rather than a
    /// prefix with nothing after it.
    #[test]
    fn a_row_too_narrow_for_one_entry_draws_nothing() {
        let buf = drawn(24, 24, &workspace());
        let row = row_text(&buf, 21);
        assert!(row.trim().is_empty(), "{row:?}");
    }

    /// The narrow scope is used, not a truncation of the wide one.
    #[test]
    fn the_narrow_scope_is_the_written_short_form() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 23);
        assert!(row.contains("214 remembered"), "{row:?}");
        assert!(!row.contains("back to 21 August"));
    }

    /// The nav abbreviates rather than wrapping, and Today is lit.
    #[test]
    fn the_nav_abbreviates() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 0);
        assert!(row.contains("Today  Week  ?"), "{row:?}");
        assert!(!row.contains("The week"));
        assert!(!row.contains("settings"));
    }

    /// Nothing scrolls sideways: no row is longer than the grid.
    #[test]
    fn nothing_runs_past_the_right_edge() {
        for width in [40u16, 60, 80, 100] {
            let buf = drawn(width, 24, &workspace());
            for row in 0..24u16 {
                assert_eq!(
                    row_text(&buf, row).chars().count(),
                    usize::from(width),
                    "row {row} is the wrong width at {width}"
                );
            }
        }
    }

    /// A terminal too short for the tail drops the collapsed lines rather than
    /// drawing them over the conversation.
    #[test]
    fn a_very_short_terminal_drops_the_tail() {
        let buf = drawn(80, 6, &workspace());
        let text = all_text(&buf);
        assert!(!text.contains("Just remembered"));
        assert!(text.contains("Keeping up"));
    }

    /// Every size draws without panicking.
    #[test]
    fn every_size_draws_without_panicking() {
        let workspace = workspace();
        for (w, h) in [(1u16, 1u16), (6, 2), (20, 8), (80, 24), (80, 40)] {
            let buf = drawn(w, h, &workspace);
            assert_eq!(buf.area.width, w);
        }
    }

    /// An empty workspace draws chrome and empty frames rather than panicking.
    #[test]
    fn an_empty_workspace_draws() {
        let buf = drawn(80, 24, &Workspace::default());
        assert_eq!(buf[(0, 1)].symbol(), "┌");
    }
}
