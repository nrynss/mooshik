//! The right-hand column: today, what keeps coming back, just remembered.
//!
//! Three panels, stacked, each answering a different question about the same
//! day — what happened, what you keep returning to, and what Mooshik has picked
//! up in the last little while. Artboard `1d` swaps the middle one for "What
//! leans on this", which is the same panel showing one thread's dependents
//! instead of the whole list.
//!
//! **The ranking is drawn twice, deliberately differently.** A thread's text
//! takes the four-step brightness ramp; its day marks do not (they are always
//! the fading step, with absences in absence). The trickle's text takes a
//! *different* ramp that bottoms out in absence rather than furniture. That is
//! not inconsistency — see [`Strength::trickle_role`]: a thread can climb back
//! up, so it never fades to the colour of an absent day, and a trickle line is
//! on its way out, so it does.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Day, Entry, Thread, Tone, Trickle},
        theme::{Role, Strength},
        widget::{marks, Kind, Panel},
        wrap::wrap,
    },
};

/// Column the timestamp gutter starts at, inside a panel's interior.
const GUTTER: u16 = 1;
/// Column entry text starts at — the design's `--cw * 82` against a panel at
/// column 72, so the gutter is the same eight cells as the conversation's.
const ENTRY_TEXT: u16 = 9;
/// Rows the ribbon takes: the dates, the bars, and a blank under them.
const RIBBON_ROWS: u16 = 3;
/// Cells kept clear at the right-hand end of the Today panel's footer.
const FOOTER_MARGIN: u16 = 2;

/// Column a thread's day marks sit at, inside the panel's interior.
const MARKS: u16 = 2;
/// Column a thread's text sits at: the marks, then two cells of gutter.
const THREAD_TEXT: u16 = MARKS + marks::WEEK as u16 + 2;
/// Column the trickle's text sits at, after its bullet.
const TRICKLE_TEXT: u16 = 4;
/// Indent of the dependency list in the "What leans on this" panel.
const LEANING_INDENT: u16 = 3;

/// Draw the Today panel: the week ribbon, the day's log, and how the day is.
pub fn today(
    grid: &mut Grid<'_>,
    day: &Day,
    week_days: &[Day],
    today_index: usize,
    focused: bool,
    at: Place,
) {
    let mut inner =
        Panel::new(text::get("tui.panel_today"), Kind::focused_if(focused)).draw(grid, at);

    let ribbon = marks::Ribbon::new(week_days, today_index);
    for (at, span) in ribbon.dates() {
        inner.run(1 + at, 0, [span]);
    }
    for (at, span) in ribbon.bars() {
        inner.run(1 + at, 1, [span]);
    }

    let last = inner.height().saturating_sub(1);
    let entries_end = entries(&mut inner, &day.entries, RIBBON_ROWS, last);

    // The footer follows the log after a blank row, as artboard `1a` places it,
    // and is pushed no further than the panel's last interior row so a long log
    // cannot write it off the bottom.
    let footer_row = entries_end.saturating_add(1).min(last);
    footer(&mut inner, day, footer_row);
}

/// Draw a timed log from `row`, stopping before `limit`. Returns the row after
/// the last line drawn.
fn entries(grid: &mut Grid<'_>, log: &[Entry], row: u16, limit: u16) -> u16 {
    let width = grid.width().saturating_sub(ENTRY_TEXT);
    let mut at = row;
    for entry in log {
        let role = match entry.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Body,
        };
        let mut first = true;
        for line in wrap(&entry.text, width) {
            if at >= limit {
                return at;
            }
            if first {
                if let Some(time) = &entry.time {
                    grid.put(GUTTER, at, &format!(" {time}"), Role::Furniture.style());
                }
                first = false;
            }
            grid.put(ENTRY_TEXT, at, &line, role.style());
            at = at.saturating_add(1);
        }
    }
    at
}

/// Draw the Today panel's footer: the weather, and how the day is going.
///
/// Both are furniture on this panel — artboard `1a` draws "Clear, 19°" and "A
/// good day so far" in the same colour — with the one exception the palette
/// insists on: a hard day keeps its caution colour wherever it appears.
fn footer(grid: &mut Grid<'_>, day: &Day, row: u16) {
    if let Some(weather) = &day.weather {
        grid.put(GUTTER, row, &format!(" {weather}"), Role::Furniture.style());
    }
    if let Some(mood) = &day.mood {
        let role = match mood.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Body,
            Tone::Plain => Role::Furniture,
        };
        let end = grid.width().saturating_sub(FOOTER_MARGIN);
        grid.put_ending_at(end, row, &mood.text, role.style());
    }
}

/// Draw "What keeps coming back": the threads, strongest first.
///
/// Only the top thread's reason is drawn. Position and brightness already rank
/// the rest, and the artboard gives the explanation to the one thread that has
/// earned it — repeating a reason under every row would be the tier list the
/// design is arguing against.
pub fn threads(grid: &mut Grid<'_>, list: &[Thread], focused: bool, at: Place) {
    let mut inner =
        Panel::new(text::get("tui.panel_threads"), Kind::focused_if(focused)).draw(grid, at);

    let width = inner.width().saturating_sub(THREAD_TEXT);
    let height = inner.height();
    let mut at = 0;

    for (rank, thread) in list.iter().enumerate() {
        let style = Strength::from_rank(rank).style();
        let mut first = true;
        for line in wrap(&thread.summary, width) {
            if at >= height {
                return;
            }
            if first {
                inner.run(MARKS, at, marks::compact(thread.days));
                first = false;
            }
            inner.put(THREAD_TEXT, at, &line, style);
            at = at.saturating_add(1);
        }
        if rank == 0 && !thread.because.is_empty() {
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
    }
}

/// Draw "What leans on this" — artboard `1d`'s replacement for the thread list.
///
/// A caution frame, because it is part of the same statement the conversation is
/// making, and one thread rather than five: the question on screen is what
/// depends on the thing about to change.
pub fn leans_on(grid: &mut Grid<'_>, thread: &Thread, at: Place) {
    let mut inner = Panel::new(text::get("tui.panel_leans"), Kind::Caution).draw(grid, at);

    let width = inner.width().saturating_sub(THREAD_TEXT);
    let height = inner.height();
    let mut at = 0;
    let mut first = true;
    for line in wrap(&thread.summary, width) {
        if at >= height {
            return;
        }
        if first {
            inner.run(MARKS, at, marks::compact(thread.days));
            first = false;
        }
        inner.put(THREAD_TEXT, at, &line, Role::Strongest.style());
        at = at.saturating_add(1);
    }

    // A blank row, then the count and the list itself.
    at = at.saturating_add(1);
    if at >= height {
        return;
    }
    let header =
        text::get("tui.leans.header").replace("{count}", &thread.leaned_on.len().to_string());
    inner.put(LEANING_INDENT, at, &header, Role::Furniture.style());
    inner.lines(
        LEANING_INDENT,
        at.saturating_add(1),
        &thread.leaned_on,
        Role::Fading.style(),
    );
}

/// Draw "Just remembered": what Mooshik has picked up, freshest first.
pub fn trickle(grid: &mut Grid<'_>, list: &[Trickle], focused: bool, at: Place) {
    let mut inner =
        Panel::new(text::get("tui.panel_trickle"), Kind::focused_if(focused)).draw(grid, at);

    let bullet = text::get("tui.trickle_bullet");
    let width = inner.width().saturating_sub(TRICKLE_TEXT);
    for (rank, line) in list.iter().enumerate() {
        let at = u16::try_from(rank).unwrap_or(u16::MAX);
        if at >= inner.height() {
            return;
        }
        // Something returning from another day takes the returning colour
        // whatever its position, because that is the only thing blue means.
        let role = if line.returned {
            Role::Returned
        } else {
            Strength::trickle_role(rank)
        };
        // The bullet is furniture, except on the rows that have faded to
        // absence — a bright bullet on a nearly-gone line would fight it.
        let bullet_role = if role == Role::Absence {
            Role::Absence
        } else {
            Role::Furniture
        };
        // One line each: the trickle is a glance, not a read, so a long entry
        // is clipped by the panel rather than pushing the next one down.
        let text = wrap(&line.text, width)
            .into_iter()
            .next()
            .unwrap_or_default();
        inner.run(
            GUTTER,
            at,
            [
                Span::styled(bullet, bullet_role.style()),
                Span::styled(text, role.style()),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, style::Style};

    use crate::tui::model::{Justification, Load, Mood};

    fn drawn(w: u16, h: u16, draw: impl FnOnce(&mut Grid<'_>)) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        draw(&mut grid);
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

    fn style_at(buf: &Buffer, col: u16, row: u16) -> Style {
        let cell = &buf[(col, row)];
        Style::default().fg(cell.fg).add_modifier(cell.modifier)
    }

    fn thread(summary: &str, days: [bool; 7], because: &str) -> Thread {
        Thread {
            summary: summary.to_owned(),
            days,
            because: Justification::history(because),
            leaned_on: Vec::new(),
        }
    }

    fn a_day() -> Day {
        Day {
            day_of_month: "27".to_owned(),
            weather: Some("Clear, 19°".to_owned()),
            mood: Some(Mood::plain("A good day so far")),
            load: Load::new(4, Tone::Notable),
            entries: vec![
                Entry::at("08:10", "Rode in, 12km"),
                Entry::at("14:20", "Call Mum back"),
            ],
            ..Day::default()
        }
    }

    /// The Today panel: the ribbon on top, the log under it, the weather and
    /// mood on the footer.
    #[test]
    fn the_today_panel_stacks_ribbon_log_and_footer() {
        let day = a_day();
        let week = vec![day.clone()];
        let buf = drawn(48, 16, |grid| {
            today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
        });
        let text = all_text(&buf);
        assert!(text.contains("27"), "the ribbon date is missing");
        assert!(text.contains("08:10"), "the log is missing");
        assert!(text.contains("Rode in, 12km"));
        assert!(text.contains("Clear, 19°"));
        assert!(text.contains("A good day so far"));
    }

    /// The log's gutter and text columns line up with the conversation's, so the
    /// two panels read as one grid.
    #[test]
    fn the_log_uses_the_same_gutter_as_the_conversation() {
        let day = a_day();
        let week = vec![day.clone()];
        let buf = drawn(48, 16, |grid| {
            today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
        });
        // Interior row 3 is buffer row 4; the gutter is buffer column 2.
        let row = row_text(&buf, 4);
        assert!(row.starts_with("│  08:10  Rode in"), "{row:?}");
        assert_eq!(ENTRY_TEXT, 9);
    }

    /// The footer never gets written off the bottom, however long the log.
    #[test]
    fn a_long_log_cannot_push_the_footer_off_the_panel() {
        let mut day = a_day();
        day.entries = (0..40)
            .map(|n| Entry::at("09:00", &format!("Entry {n}")))
            .collect();
        let week = vec![day.clone()];
        let buf = drawn(48, 16, |grid| {
            today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
        });
        assert!(all_text(&buf).contains("Clear, 19°"), "the footer was lost");
        // And the frame is intact — nothing wrote over the bottom rule.
        assert_eq!(buf[(0, 15)].symbol(), "└");
    }

    /// A hard day keeps its caution colour on the footer; an ordinary one is
    /// furniture, as artboard `1a` draws it.
    #[test]
    fn only_a_hard_day_colours_the_footer() {
        let mut day = a_day();
        let week = vec![day.clone()];
        let buf = drawn(48, 16, |grid| {
            today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
        });
        let plain_row = find_row(&buf, "A good day so far");
        assert_eq!(
            style_at(&buf, col_of(&buf, plain_row, "A good day"), plain_row),
            Role::Furniture.style()
        );

        day.mood = Some(Mood::hard("A rough day"));
        let buf = drawn(48, 16, |grid| {
            today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
        });
        let hard_row = find_row(&buf, "A rough day");
        assert_eq!(
            style_at(&buf, col_of(&buf, hard_row, "A rough day"), hard_row),
            Role::Caution.style()
        );
    }

    fn find_row(buf: &Buffer, needle: &str) -> u16 {
        (0..buf.area.height)
            .find(|r| row_text(buf, *r).contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen"))
    }

    /// The character column `needle` starts at. `str::find` gives a byte offset
    /// and these rows carry multi-byte glyphs, so the two are not the same.
    fn col_of(buf: &Buffer, row: u16, needle: &str) -> u16 {
        let line = row_text(buf, row);
        let byte = line
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} is not on row {row}"));
        u16::try_from(line[..byte].chars().count()).expect("within the grid")
    }

    /// The thread list ranks by brightness, strongest first, and only the top
    /// thread carries its reason.
    #[test]
    fn only_the_top_thread_shows_its_reason() {
        let list = vec![
            thread("First thought", [true; 7], "Every day this week"),
            thread(
                "Second thought",
                [true, false, true, true, true, true, true],
                "Five days",
            ),
        ];
        let buf = drawn(48, 14, |grid| {
            threads(grid, &list, false, Place::new(0, 0, 48, 14))
        });
        let text = all_text(&buf);
        assert!(text.contains("Every day this week"));
        assert!(
            !text.contains("Five days"),
            "a lower thread showed its reason"
        );
    }

    /// Brightness steps down the list, and the marks do not step with it.
    #[test]
    fn the_text_ranks_but_the_marks_do_not() {
        let list: Vec<Thread> = (0..3)
            .map(|n| thread(&format!("Thought {n}"), [true; 7], ""))
            .collect();
        let buf = drawn(48, 14, |grid| {
            threads(grid, &list, false, Place::new(0, 0, 48, 14))
        });
        // Interior rows 0, 1, 2 are buffer rows 1, 2, 3; text is at buffer
        // column 1 + THREAD_TEXT.
        let text_col = 1 + THREAD_TEXT;
        assert_eq!(style_at(&buf, text_col, 1), Role::Strongest.style());
        assert_eq!(style_at(&buf, text_col, 2), Role::Body.style());
        assert_eq!(style_at(&buf, text_col, 3), Role::Fading.style());
        // Every mark row is the same colour, whatever its rank.
        let mark_col = 1 + MARKS;
        for row in 1..=3u16 {
            assert_eq!(style_at(&buf, mark_col, row), Role::Fading.style());
        }
    }

    /// A thread whose reason is that it just came back is drawn in the returning
    /// colour — the one thing blue means.
    #[test]
    fn a_returning_reason_is_blue() {
        let mut list = vec![thread("The 512 cap", [true; 7], "")];
        list[0].because = Justification::came_back("Came back just now");
        let buf = drawn(48, 14, |grid| {
            threads(grid, &list, false, Place::new(0, 0, 48, 14))
        });
        let row = find_row(&buf, "Came back just now");
        assert_eq!(style_at(&buf, 1 + THREAD_TEXT, row), Role::Returned.style());
    }

    /// The "what leans on this" panel is a caution frame carrying one thread and
    /// its dependents, with the count in the header.
    #[test]
    fn the_leans_panel_is_a_caution_frame_with_a_count() {
        let mut one = thread("Block, never drop", [true; 7], "");
        one.leaned_on = vec![
            "The short postmortem".to_owned(),
            "The oncall runbook".to_owned(),
        ];
        let buf = drawn(48, 14, |grid| {
            leans_on(grid, &one, Place::new(0, 0, 48, 14))
        });
        assert_eq!(style_at(&buf, 0, 0), Role::Caution.style());
        let text = all_text(&buf);
        assert!(text.contains("What leans on this"));
        assert!(text.contains("2 things lean on it:"), "{text}");
        assert!(text.contains("The oncall runbook"));
    }

    /// The trickle's ramp bottoms out in absence, and its bullet follows the
    /// line down rather than staying bright over a nearly-gone entry.
    #[test]
    fn the_trickle_fades_to_absence_and_its_bullet_follows() {
        let list: Vec<Trickle> = (0..5).map(|n| Trickle::new(&format!("Line {n}"))).collect();
        let buf = drawn(48, 7, |grid| {
            trickle(grid, &list, false, Place::new(0, 0, 48, 7))
        });
        let text_col = 1 + TRICKLE_TEXT;
        assert_eq!(style_at(&buf, text_col, 1), Role::Body.style());
        assert_eq!(style_at(&buf, text_col, 2), Role::Fading.style());
        assert_eq!(style_at(&buf, text_col, 3), Role::Furniture.style());
        assert_eq!(style_at(&buf, text_col, 4), Role::Absence.style());
        // The bullet sits at interior column 1 — buffer column 2.
        assert_eq!(style_at(&buf, 2, 1), Role::Furniture.style());
        assert_eq!(style_at(&buf, 2, 4), Role::Absence.style());
    }

    /// A returning trickle line is blue wherever it sits in the list.
    #[test]
    fn a_returning_trickle_line_is_blue_at_any_position() {
        let list = vec![
            Trickle::new("Newest"),
            Trickle::came_back("Brought back Monday's decision"),
            Trickle::new("Older"),
        ];
        let buf = drawn(48, 7, |grid| {
            trickle(grid, &list, false, Place::new(0, 0, 48, 7))
        });
        assert_eq!(style_at(&buf, 1 + TRICKLE_TEXT, 2), Role::Returned.style());
    }

    /// A trickle longer than its panel stops at the bottom rule rather than
    /// writing over it.
    #[test]
    fn an_overlong_trickle_stops_at_the_rule() {
        let list: Vec<Trickle> = (0..20)
            .map(|n| Trickle::new(&format!("Line {n}")))
            .collect();
        let buf = drawn(48, 7, |grid| {
            trickle(grid, &list, false, Place::new(0, 0, 48, 7))
        });
        assert_eq!(buf[(0, 6)].symbol(), "└");
        assert!(!all_text(&buf).contains("Line 6"));
    }

    /// One trickle entry is one row: a long line is clipped rather than pushing
    /// the entries under it down.
    #[test]
    fn a_long_trickle_entry_takes_one_row() {
        let list = vec![
            Trickle::new(
                "A very long thing that will not fit on one row of this narrow panel at all",
            ),
            Trickle::new("Second"),
        ];
        let buf = drawn(48, 7, |grid| {
            trickle(grid, &list, false, Place::new(0, 0, 48, 7))
        });
        assert!(
            row_text(&buf, 2).contains("Second"),
            "{:?}",
            row_text(&buf, 2)
        );
    }

    /// Focus moves the rule to the accent on whichever panel has it.
    #[test]
    fn focus_accents_the_panel_rule() {
        let list = vec![thread("x", [true; 7], "")];
        let buf = drawn(48, 6, |grid| {
            threads(grid, &list, true, Place::new(0, 0, 48, 6))
        });
        assert_eq!(style_at(&buf, 0, 0), Role::Accent.style());
        let buf = drawn(48, 6, |grid| {
            threads(grid, &list, false, Place::new(0, 0, 48, 6))
        });
        assert_eq!(style_at(&buf, 0, 0), Role::Furniture.style());
    }
}
