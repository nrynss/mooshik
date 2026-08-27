//! The week — artboard `1b`.
//!
//! "Seven days, and the threads that run across them — strength reads as
//! brightness, day-marks and order, never a tier."
//!
//! Two things distinguish this screen from the Today panel's version of the same
//! data, and both come straight from `1i`:
//!
//! * The day marks are **aligned under their day columns** rather than packed
//!   adjacent, "so a thread lines up with the days it belongs to". The header
//!   row `Fri Sat Sun …` is what they line up against, which is why it is drawn
//!   here and not in [`marks`](crate::tui::widget::marks).
//! * **Every thread shows its reason**, where the Today panel gives one only to
//!   the thread at the top. There is room here, and this is the screen someone
//!   opens to ask why — so "Five days · you checked it again today" earns its
//!   line.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Day, Entry, Thread, Tone, Week, Workspace},
        theme::{Role, Strength},
        widget::{marks, Kind, Panel},
        wrap::{wrap, wrap_paragraphs},
    },
};

use super::{chrome, Band};

/// Days across the top.
const DAYS: u16 = 7;
/// Rows the day columns take.
const DAY_ROWS: u16 = 15;
/// The column the detail pane starts at on a 120-column screen, as a proportion.
const DETAIL_NUMERATOR: u16 = 74;
const DETAIL_DENOMINATOR: u16 = 120;

/// Column the day header and thread marks start at, inside the thread panel.
/// The header's own two leading spaces put `Fri` at column 3, and the marks at
/// column 4 land in the middle of it.
const DAY_HEADER: u16 = 1;
const THREAD_MARKS: u16 = 4;
/// Column thread text starts at: past seven marks at a stride of four.
const THREAD_TEXT: u16 = 33;

/// Column an entry's time sits at in the detail pane, and its text.
const DETAIL_TIME: u16 = 1;
const DETAIL_TEXT: u16 = 8;

/// Draw the week screen over the whole of `grid`.
pub fn draw(grid: &mut Grid<'_>, workspace: &Workspace, thread_cursor: usize) {
    let band = Band::new(grid.height(), chrome::Margins::WIDE);
    let width = grid.width();

    let subject = format!(
        "{}{}{}",
        text::get("tui.week_title"),
        text::get("tui.separator"),
        workspace.week.label
    );
    chrome::title(grid, band.margins, &subject, chrome::View::Week);

    let day_rows = DAY_ROWS.min(band.rows());
    columns(grid, &workspace.week, band.top, width, day_rows);

    let lower_top = band.top.saturating_add(day_rows);
    let lower_rows = band.rows().saturating_sub(day_rows);
    let detail_col = (width.saturating_mul(DETAIL_NUMERATOR) / DETAIL_DENOMINATOR).max(1);

    threads(
        grid,
        &workspace.threads,
        thread_cursor,
        Place::new(0, lower_top, detail_col, lower_rows),
    );
    detail(
        grid,
        workspace.week.selected_day(),
        Place::new(
            detail_col,
            lower_top,
            width.saturating_sub(detail_col),
            lower_rows,
        ),
    );

    // Both runs on this rule are left-aligned: the keys at the margin, and the
    // scope under the pane it sits beside, so the rule reads as a continuation
    // of the two columns above it. Right-aligning either would put them on a
    // collision course as the terminal narrows — and did.
    chrome::note_rule(
        grid,
        band.margins.left,
        band.status,
        text::get("tui.hint_week"),
    );
    // The short scope, because this rule already carries the week's own label
    // and the long form ("…, back to 21 August") would run off the edge.
    chrome::note_rule(
        grid,
        detail_col,
        band.status,
        &format!(
            "{}{}{}",
            workspace.week.label,
            text::get("tui.separator"),
            workspace.health.short_scope
        ),
    );
}

/// Draw the seven day columns across the top.
fn columns(grid: &mut Grid<'_>, week: &Week, row: u16, width: u16, height: u16) {
    let each = (width / DAYS).max(1);
    for (index, day) in week.days.iter().take(usize::from(DAYS)).enumerate() {
        let at = u16::try_from(index).unwrap_or(0).saturating_mul(each);
        // The last column takes whatever the division left over, so the row of
        // panels reaches the right edge instead of leaving a gap that reads as a
        // rendering fault.
        let is_last = index + 1 == week.days.len().min(usize::from(DAYS));
        let panel_width = if is_last {
            width.saturating_sub(at)
        } else {
            each
        };
        column(
            grid,
            day,
            index == week.selected,
            Place::new(at, row, panel_width, height),
        );
    }
}

/// Draw one day column: the weather, what happened, and how the day felt.
fn column(grid: &mut Grid<'_>, day: &Day, selected: bool, at: Place) {
    // The title is a date, so it takes the date colour — except on the selected
    // day and on a day whose mood is worth noticing, where it brightens.
    let title_role = if selected || matches!(day.mood.as_ref().map(|m| m.tone), Some(Tone::Notable))
    {
        Role::Strongest
    } else {
        Role::Date
    };
    let mut inner = Panel::new(&day.short_label, Kind::focused_if(selected))
        .titled_as(title_role)
        .draw(grid, at);

    let width = inner.width();
    let last = inner.height().saturating_sub(1);
    if let Some(weather) = &day.weather {
        inner.put(0, 0, weather, Role::Furniture.style());
    }

    let mut at = 2;
    for entry in &day.highlights {
        let role = match entry.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Fading,
        };
        for line in wrap(&entry.text, width) {
            if at >= last {
                break;
            }
            inner.put(0, at, &line, role.style());
            at = at.saturating_add(1);
        }
    }

    if let Some(mood) = &day.mood {
        let role = match mood.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Body,
        };
        let mood_row = at.saturating_add(1).min(last);
        inner.put(0, mood_row, &mood.text, role.style());
    }
}

/// Draw "What keeps coming back" with the marks aligned under their days.
fn threads(grid: &mut Grid<'_>, list: &[Thread], cursor: usize, at: Place) {
    let mut inner = Panel::new(text::get("tui.panel_threads"), Kind::Idle).draw(grid, at);

    inner.put(
        DAY_HEADER,
        0,
        text::get("tui.week_day_header"),
        Role::Furniture.style(),
    );

    let width = inner.width().saturating_sub(THREAD_TEXT);
    let height = inner.height();
    let mut at = 1;

    for (rank, thread) in list.iter().enumerate() {
        // The cursor brightens the row it is on without moving it, so `J`/`K`
        // never reorders a list whose order is its meaning.
        let style = if rank == cursor {
            Role::Strongest.style()
        } else {
            Strength::from_rank(rank).style()
        };
        let mut first = true;
        for line in wrap(&thread.summary, width) {
            if at >= height {
                return;
            }
            if first {
                inner.run(THREAD_MARKS, at, marks::aligned(thread.days, style));
                first = false;
            }
            inner.put(THREAD_TEXT, at, &line, style);
            at = at.saturating_add(1);
        }
        if !thread.because.is_empty() {
            let role = if thread.because.returned {
                Role::Returned
            } else {
                Role::Furniture
            };
            for line in wrap(&thread.because.text, width) {
                if at >= height {
                    return;
                }
                inner.put(THREAD_TEXT, at, &line, role.style());
                at = at.saturating_add(1);
            }
        }
        // A blank row between threads: with every reason drawn, the rows would
        // otherwise run together into one block of text.
        at = at.saturating_add(1);
    }
}

/// Draw the selected day in full: its log, and what Mooshik noticed about it.
fn detail(grid: &mut Grid<'_>, day: Option<&Day>, at: Place) {
    let title = day.map_or("", |d| d.long_label.as_str());
    let mut inner = Panel::new(title, Kind::Focused).draw(grid, at);
    let Some(day) = day else { return };

    let height = inner.height();
    if height == 0 {
        return;
    }

    // The weather and the mood share the head of the pane, the mood keeping its
    // own tone so a hard day is yellow here as it is everywhere else.
    let mut head = Vec::new();
    if let Some(weather) = &day.weather {
        head.push(Span::styled(
            format!("{weather}{}", text::get("tui.separator")),
            Role::Furniture.style(),
        ));
    }
    if let Some(mood) = &day.mood {
        let role = match mood.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Body,
        };
        head.push(Span::styled(mood.text.clone(), role.style()));
    }
    inner.run(DETAIL_TIME, 0, head);

    let width = inner.width().saturating_sub(DETAIL_TEXT);
    let mut at = 2;
    for entry in day.detail_entries() {
        at = log_entry(&mut inner, &entry, at, width, height);
        if at >= height {
            return;
        }
    }

    if !day.notes.trim().is_empty() {
        at = at.saturating_add(1);
        let notes_width = inner.width().saturating_sub(DETAIL_TIME);
        for line in wrap_paragraphs(&day.notes, notes_width) {
            if at >= height {
                return;
            }
            inner.put(DETAIL_TIME, at, &line, Role::Fading.style());
            at = at.saturating_add(1);
        }
    }
}

/// Draw one logged entry with a hanging indent, returning the row after it.
fn log_entry(grid: &mut Grid<'_>, entry: &Entry, row: u16, width: u16, limit: u16) -> u16 {
    let role = match entry.tone {
        Tone::Hard => Role::Caution,
        Tone::Notable => Role::Strongest,
        Tone::Plain => Role::Body,
    };
    let mut at = row;
    let mut first = true;
    for line in wrap(&entry.text, width) {
        if at >= limit {
            return at;
        }
        if first {
            if let Some(time) = &entry.time {
                grid.put(DETAIL_TIME, at, time, Role::Furniture.style());
            }
            first = false;
        }
        grid.put(DETAIL_TEXT, at, &line, role.style());
        at = at.saturating_add(1);
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, style::Style};

    use crate::tui::model::{Health, Justification, Load, Mood};

    fn day(label: &str, of_month: &str, hard: bool) -> Day {
        Day {
            short_label: label.to_owned(),
            long_label: format!("{label} August"),
            day_of_month: of_month.to_owned(),
            weather: Some("Rain · 15°".to_owned()),
            mood: Some(if hard {
                Mood::hard("A rough day")
            } else {
                Mood::plain("Steady")
            }),
            load: Load::new(4, Tone::Plain),
            highlights: vec![Entry::line("Moved the cache")],
            entries: vec![Entry::at("09:42", "The ring overflowed in production")],
            notes: "You came back to this four times.\n\nAnd again since.".to_owned(),
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            week: Week {
                label: "21-27 August".to_owned(),
                days: (0..7)
                    .map(|n| day(&format!("Day {n}"), &format!("2{n}"), n == 5))
                    .collect(),
                selected: 5,
            },
            threads: (0..3)
                .map(|n| Thread {
                    summary: format!("Thought number {n}"),
                    days: [true, true, false, true, true, true, false],
                    because: Justification::history("Five days this week"),
                    leaned_on: Vec::new(),
                })
                .collect(),
            health: Health {
                state: "Keeping up".to_owned(),
                scope: "214 things remembered".to_owned(),
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
        draw(&mut grid, workspace, 0);
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

    /// The character column `needle` starts at on `row`.
    ///
    /// `str::find` returns a byte offset, and these rows are full of multi-byte
    /// glyphs — `│`, `·`, `°` are three, two and two bytes — so the byte offset
    /// is not the column. Counting characters up to the match is.
    fn col_of(buf: &Buffer, row: u16, needle: &str) -> u16 {
        let line = row_text(buf, row);
        let byte = line
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} is not on row {row}"));
        u16::try_from(line[..byte].chars().count()).expect("within the grid")
    }

    fn row_of(buf: &Buffer, rows: std::ops::Range<u16>, needle: &str) -> u16 {
        rows.clone()
            .find(|r| row_text(buf, *r).contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not in rows {rows:?}"))
    }

    fn style_at(buf: &Buffer, col: u16, row: u16) -> Style {
        let cell = &buf[(col, row)];
        Style::default().fg(cell.fg).add_modifier(cell.modifier)
    }

    /// The artboard's geometry: seven columns 15 rows tall, then the thread
    /// panel and the detail pane split at column 74 of 120.
    #[test]
    fn the_geometry_matches_the_artboard() {
        let buf = drawn(120, 40, &workspace());
        // Seven day panels, each starting on a 17-column stride.
        for index in 0..7u16 {
            let col = index * 17;
            assert_eq!(buf[(col, 1)].symbol(), "┌", "no day panel at column {col}");
        }
        // The lower panels start on row 16 and split at column 74.
        assert_eq!(buf[(0, 16)].symbol(), "┌");
        assert_eq!(buf[(74, 16)].symbol(), "┌");
    }

    /// The marks line up under their day headers, which is `1i`'s requirement
    /// for this screen and the reason the header is drawn here.
    #[test]
    fn the_marks_line_up_under_their_day_headers() {
        let buf = drawn(120, 40, &workspace());
        let header_row = (0..40u16)
            .find(|r| row_text(&buf, *r).contains("Fri Sat Sun"))
            .expect("the day header is drawn");
        let fri = col_of(&buf, header_row, "Fri");
        let thu = col_of(&buf, header_row, "Thu");
        let marks_row: Vec<char> = row_text(&buf, header_row + 1).chars().collect();
        // The mark sits on the middle character of its three-letter header.
        assert_eq!(
            marks_row.get(usize::from(fri) + 1),
            Some(&'▇'),
            "the first mark is not under Fri"
        );
        // And so does the last day's, present or absent.
        assert!(matches!(
            marks_row.get(usize::from(thu) + 1),
            Some('▇' | '·')
        ));
    }

    /// Every thread shows its reason on this screen, unlike the Today panel.
    #[test]
    fn every_thread_shows_its_reason_here() {
        let buf = drawn(120, 40, &workspace());
        let text = all_text(&buf);
        assert_eq!(
            text.matches("Five days this week").count(),
            3,
            "not every thread's reason is drawn"
        );
    }

    /// The cursor brightens a row without moving it — the list's order is its
    /// meaning, so `J`/`K` must not reorder it.
    #[test]
    fn the_cursor_brightens_without_reordering() {
        let workspace = workspace();
        let first = drawn(120, 40, &workspace);
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        draw(&mut grid, &workspace, 2);

        // The rows do not move.
        assert_eq!(
            row_of(&first, 0..40, "Thought number 0"),
            row_of(&buf, 0..40, "Thought number 0")
        );
        // But the brightest row does.
        let third = row_of(&buf, 0..40, "Thought number 2");
        assert_eq!(
            style_at(&buf, 1 + THREAD_TEXT, third),
            Role::Strongest.style()
        );
    }

    /// The selected day is the focused frame, and a hard day keeps its caution
    /// colour in the column and in the detail pane.
    #[test]
    fn the_selected_day_is_focused_and_a_hard_day_stays_yellow() {
        let buf = drawn(120, 40, &workspace());
        // Day 5 is selected — its panel starts at column 85.
        assert_eq!(style_at(&buf, 85, 1), Role::Accent.style());
        assert_eq!(style_at(&buf, 0, 1), Role::Furniture.style());
        let text = all_text(&buf);
        assert!(text.contains("A rough day"));
        let mood_row = row_of(&buf, 0..16, "A rough day");
        let col = col_of(&buf, mood_row, "A rough day");
        assert_eq!(style_at(&buf, col, mood_row), Role::Caution.style());
    }

    /// A day column's title is a date, so it takes the date colour rather than
    /// the panel's own — except where it brightens for selection.
    #[test]
    fn day_titles_are_dates() {
        let buf = drawn(120, 40, &workspace());
        // "Day 0" is not selected and its mood is plain, so its title is cyan.
        assert_eq!(buf[(3, 1)].fg, Role::Date.color());
        // The selected day's title brightens.
        assert_eq!(buf[(88, 1)].fg, Role::Strongest.color());
    }

    /// The two runs on the bottom rule do not overlap, at any width. They did:
    /// the keys were right-aligned into the scope's column.
    #[test]
    fn the_bottom_rule_does_not_collide_with_itself() {
        for width in [100u16, 120, 160, 200] {
            let buf = drawn(width, 40, &workspace());
            let rule = row_text(&buf, 39);
            assert!(
                rule.contains("H/L a day · J/K a thread · Enter open the day"),
                "the keys are cut off at {width}: {rule:?}"
            );
            assert!(
                rule.contains("214 remembered"),
                "the scope is cut off at {width}: {rule:?}"
            );
            assert!(
                !rule.contains("back to 21 August"),
                "the long scope is on the narrow rule at {width}"
            );
        }
    }

    /// The detail pane shows the selected day's log and its trailing notes, with
    /// the blank line between the notes preserved.
    #[test]
    fn the_detail_pane_shows_the_log_and_the_notes() {
        let buf = drawn(120, 40, &workspace());
        let text = all_text(&buf);
        assert!(text.contains("Day 5 August"), "the pane title is missing");
        assert!(text.contains("09:42"));
        assert!(text.contains("The ring overflowed in"));
        assert!(text.contains("You came back to this four"));
        assert!(text.contains("And again since."));
    }

    /// An empty week draws its frames and nothing else, rather than panicking.
    #[test]
    fn an_empty_week_still_draws() {
        let mut workspace = workspace();
        workspace.week.days.clear();
        workspace.threads.clear();
        let buf = drawn(120, 40, &workspace);
        assert_eq!(buf[(0, 16)].symbol(), "┌");
        assert_eq!(buf[(74, 16)].symbol(), "┌");
    }

    /// A week with an out-of-range selection leaves the detail pane empty rather
    /// than panicking on the index.
    #[test]
    fn an_out_of_range_selection_leaves_the_pane_empty() {
        let mut workspace = workspace();
        workspace.week.selected = 99;
        let buf = drawn(120, 40, &workspace);
        assert_eq!(buf[(74, 16)].symbol(), "┌");
        assert!(!all_text(&buf).contains("09:42"));
    }

    /// The columns reach the right edge at any width — a trailing gap inside the
    /// row of panels would read as a rendering fault.
    #[test]
    fn the_columns_reach_the_right_edge() {
        for width in [70u16, 100, 120, 137, 200] {
            let mut workspace = workspace();
            workspace.week.selected = 0;
            let buf = drawn(width, 40, &workspace);
            let last = width - 1;
            assert_eq!(
                buf[(last, 1)].symbol(),
                "┐",
                "the day columns leave a gap at width {width}"
            );
        }
    }

    /// Every size draws without panicking.
    #[test]
    fn every_size_draws_without_panicking() {
        let workspace = workspace();
        for (w, h) in [
            (1u16, 1u16),
            (8, 4),
            (40, 12),
            (80, 24),
            (120, 40),
            (240, 80),
        ] {
            let buf = drawn(w, h, &workspace);
            assert_eq!(buf.area.width, w);
        }
    }

    /// More than seven days in the model draws seven columns, not eight.
    #[test]
    fn only_seven_days_are_drawn() {
        let mut workspace = workspace();
        workspace.week.days.push(day("Day 7", "28", false));
        let buf = drawn(120, 40, &workspace);
        assert!(!all_text(&buf).contains("Day 7"));
    }
}
