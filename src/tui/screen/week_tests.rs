//! The week screen's own tests, in a sibling file.
//!
//! Split out of `week.rs` to keep both inside the ~600-line soft target from
//! `README.md`, the same way `screen/tests.rs` and `cli/tests.rs` are separate
//! files. Nothing else moved: these are `week.rs`'s tests, reached through a
//! `#[path]` module there, so `super::*` still names its private items.

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
        entries: vec![Entry::at("09:42", "The ring overflowed in production").hard()],
        notes: "You came back to this four times.\n\nAnd again since.".to_owned(),
    }
}

/// A day worth noticing rather than a hard one — the third mood tone, which
/// brightens a column's title without spending the caution colour.
fn notable_day(label: &str, of_month: &str) -> Day {
    Day {
        mood: Some(Mood::notable("Better")),
        ..day(label, of_month, false)
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

/// The lower split keeps its 74-of-120 proportion at every width, including the
/// wide ones where `width * 74` leaves `u16`.
///
/// It used to be a *saturating* `u16` multiply, so past 886 columns the product
/// pinned at `u16::MAX` and the division gave a constant 546 — the detail pane
/// swallowed everything to its right.
#[test]
fn the_lower_split_keeps_its_proportion_at_every_width() {
    for width in [40u16, 80, 120, 400, 886, 887, 2000, u16::MAX] {
        let expected = u16::try_from(u32::from(width) * 74 / 120).unwrap().max(1);
        let buf = drawn(width, 40, &workspace());
        assert_eq!(
            buf[(expected, 16)].symbol(),
            "┌",
            "the detail pane is not at column {expected} of {width}"
        );
        // And the thread panel to its left is not a sliver.
        assert!(expected > width - expected, "the split inverted at {width}");
    }
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

/// A notable mood brightens a day's title and its own line without reaching for
/// the caution colour, which `1i` allows twice a week at most.
#[test]
fn a_notable_day_brightens_without_spending_the_caution_colour() {
    let mut workspace = workspace();
    // Day 0 is unselected, so any brightening is the mood's doing.
    workspace.week.days[0] = notable_day("Day 0", "21");
    let buf = drawn(120, 40, &workspace);

    // The title brightens out of the cyan spine.
    assert_eq!(buf[(3, 1)].fg, Role::Strongest.color());
    // And so does the mood, in the strongest step rather than in yellow.
    let mood_row = row_of(&buf, 0..16, "Better");
    let col = col_of(&buf, mood_row, "Better");
    assert_eq!(style_at(&buf, col, mood_row), Role::Strongest.style());
    assert_ne!(style_at(&buf, col, mood_row), Role::Caution.style());
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

/// The two runs on the bottom rule do not overlap, at any width.
///
/// They did, and 80 and 100 were exactly the widths that showed it: the scope's
/// column is a proportion of the width, and below about 108 it landed inside the
/// keys, which are a fixed 47 characters from the left margin. The old version
/// of this test asserted on a *prefix* of the hint and only from 100 columns up,
/// so it passed while the rendered rule read
/// `H/L a day · J/K a thread · ^1 today ·21-27 August  ·  214 remembered`. It
/// now asserts the whole hint, and covers 80 and 90.
#[test]
fn the_bottom_rule_does_not_collide_with_itself() {
    let keys = crate::text::get("tui.hint_week");
    for width in [80u16, 90, 100, 120, 160, 200] {
        let buf = drawn(width, 40, &workspace());
        let rule = row_text(&buf, 39);
        assert!(
            rule.contains(keys),
            "the keys are cut off or overwritten at {width}: {rule:?}"
        );
        assert!(
            rule.contains("21-27 August · 214 remembered"),
            "the scope is cut off at {width}: {rule:?}"
        );
        assert!(
            !rule.contains("back to 21 August"),
            "the long scope is on the narrow rule at {width}"
        );
    }
}

/// A rule with no room for both runs keeps the keys whole and drops the scope,
/// rather than writing the scope over them.
#[test]
fn a_rule_too_narrow_for_both_runs_drops_the_scope() {
    let buf = drawn(60, 40, &workspace());
    let rule = row_text(&buf, 39);
    assert!(rule.contains("H/L a day · J/K a thread"), "{rule:?}");
    assert!(!rule.contains("214 remembered"), "{rule:?}");
}

/// The detail pane with no day selected draws no title, so its accent rule is
/// whole. A padded empty title punched three ground-coloured cells into it.
#[test]
fn an_unselected_detail_pane_keeps_its_rule_whole() {
    let mut workspace = workspace();
    workspace.week.selected = 99;
    let buf = drawn(120, 40, &workspace);
    // The pane's top rule runs from column 74; the title would have sat at 76.
    for col in 74..120u16 {
        assert_eq!(
            buf[(col, 16)].bg,
            ratatui::style::Color::Reset,
            "column {col} of the untitled pane's rule is painted"
        );
        assert_eq!(
            buf[(col, 16)].symbol(),
            if col == 74 {
                "┌"
            } else if col == 119 {
                "┐"
            } else {
                "─"
            },
            "column {col} of the untitled pane's rule is broken"
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
    // `1b`'s detail head is `Rain, 15° · A rough day` — the tight separator,
    // not the title rule's spaced one.
    assert!(
        text.contains("Rain · 15° · A rough day"),
        "the detail head uses the wrong separator"
    );
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
