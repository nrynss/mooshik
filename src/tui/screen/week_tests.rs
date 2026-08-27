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
        weather: Some("Rain, 15°".to_owned()),
        mood: Some(if hard {
            Mood::hard("A rough day")
        } else {
            Mood::plain("Steady")
        }),
        load: Load::new(4, Tone::Plain),
        highlights: vec![Entry::line("Moved the cache")],
        // Plain, as `1b` draws it: the incident is already named in yellow in the
        // week gutter one panel to the left, and `1i` allows the caution colour
        // twice a week.
        entries: vec![Entry::at("09:42", "The ring overflowed in production")],
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

/// This panel has no right margin, and `1b`'s own longest lines are why.
///
/// It has 39 columns past `THREAD_TEXT` and spends all of them: `Three days ·
/// Monday, Tuesday, Thursday` is 38 characters on one line in the artboard, and
/// `Cobalt Lantern retries failed fetches` is 37. The shared two-cell margin cut
/// the available width to 37 and broke the first of those after `Tuesday,`.
#[test]
fn the_thread_panel_spends_its_whole_width() {
    let mut workspace = workspace();
    workspace.threads = vec![Thread {
        summary: "Cobalt Lantern retries failed fetches".to_owned(),
        days: [false, false, false, false, true, true, false],
        because: Justification::history("Three days · Monday, Tuesday, Thursday"),
        leaned_on: Vec::new(),
    }];
    let buf = drawn(120, 40, &workspace);
    let row = row_of(&buf, 16..39, "Three days");
    let line = row_text(&buf, row);
    assert!(
        line.contains("Three days · Monday, Tuesday, Thursday"),
        "the reason was broken: {line:?}"
    );
    // And the summary above it is whole too, on one line.
    let above = row_text(&buf, row - 1);
    assert!(
        above.contains("Cobalt Lantern retries failed fetches"),
        "the summary was broken: {above:?}"
    );
}

/// The detail pane's notes keep the artboard's breaks, which need five cells of
/// margin rather than the shared two.
///
/// `1b` breaks `You came back to the 512 cap four / times on this day.` after
/// `four` and `You still haven't called him back — / it's come up on two days
/// since.` after the dash. Both fit one more word at 41 columns, so both reflowed.
#[test]
fn the_detail_notes_keep_the_artboards_breaks() {
    let mut workspace = workspace();
    workspace.week.days[5].notes = "You came back to the 512 cap four times on this day.\n\n         You still haven't called him back — it's come up on two days since."
        .to_owned();
    let buf = drawn(120, 40, &workspace);
    let text = all_text(&buf);
    for (line, spilled) in [
        ("You came back to the 512 cap four", "four times"),
        ("times on this day.", ""),
        ("You still haven't called him back —", "back — it's"),
        ("it's come up on two days since.", ""),
    ] {
        assert!(text.contains(line), "{line:?} is not a line: {text}");
        if !spilled.is_empty() {
            assert!(
                !text.contains(spilled),
                "the note ran on past the artboard's break: {spilled:?}"
            );
        }
    }
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

/// The two runs on the bottom rule do not overlap, at any width, and the keys
/// are never the run that gets cut.
///
/// They did overlap, and 80 and 100 were exactly the widths that showed it: the
/// scope's column is a proportion of the width, and below about 108 it landed
/// inside the keys, which are a fixed 47 characters from the left margin. An
/// earlier version of this test asserted on a *prefix* of the hint and only from
/// 100 columns up, so it passed while the rendered rule read
/// `H/L a day · J/K a thread · ^1 today ·21-27 August  ·  214 remembered`.
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
        // Nothing reaches the chrome's right edge, whichever runs were drawn.
        let end = rule.trim_end().chars().count();
        assert!(
            end <= usize::from(width - chrome::Margins::WIDE.right),
            "the rule runs into the margin at {width}: {end} of {width}"
        );
    }
}

/// The scope prefers the long form and falls back to the written short one, which
/// is what `1b` draws: `21-27 August · 214 things remembered` from column 74 to
/// column 110 of 120. The comment here used to claim the long form would run off
/// the edge, and the short one was used unconditionally as a result.
#[test]
fn the_rule_prefers_the_long_scope_where_it_fits() {
    let buf = drawn(120, 40, &workspace());
    let rule = row_text(&buf, 39);
    assert!(
        rule.contains("21-27 August · 214 things remembered"),
        "the long scope is not on the wide rule: {rule:?}"
    );
    assert_eq!(col_of(&buf, 39, "21-27 August"), 74, "{rule:?}");

    // At 100 the long form no longer fits and the short one does.
    let buf = drawn(100, 40, &workspace());
    let rule = row_text(&buf, 39);
    assert!(
        rule.contains("21-27 August · 214 remembered"),
        "the short scope is missing at 100: {rule:?}"
    );
    assert!(!rule.contains("214 things remembered"), "{rule:?}");
}

/// A rule with no room for both runs keeps the keys whole and drops the scope,
/// rather than writing the scope over them — at 80 and 90 as well as at 60,
/// because the scope is now measured against the chrome's own right edge.
#[test]
fn a_rule_too_narrow_for_both_runs_drops_the_scope() {
    for width in [60u16, 80, 90] {
        let buf = drawn(width, 40, &workspace());
        let rule = row_text(&buf, 39);
        assert!(rule.contains("H/L a day · J/K a thread"), "{rule:?}");
        assert!(
            !rule.contains("214 remembered") && !rule.contains("214 things"),
            "the scope was drawn into the margin at {width}: {rule:?}"
        );
    }
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
    // `1b`'s detail head is `Rain, 15° · A rough day` — the tight separator, and
    // the weather itself written with a comma so it composes into it.
    assert!(
        text.contains("Rain, 15° · A rough day"),
        "the detail head does not read as the artboard's"
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

/// A terminal too narrow for seven 17-cell columns shows fewer whole days rather
/// than seven shredded ones, and says on the bottom rule how many it dropped.
///
/// `1b` spends all 13 text columns of a day's 17 cells — "Mum called /
/// mid-incident / — not called / back" is a wrap at 13 — so dividing the width by
/// seven whatever the width was gave an 80-column terminal seven 11-cell columns.
#[test]
fn a_narrow_terminal_windows_the_days_rather_than_narrowing_them() {
    let buf = drawn(80, 40, &workspace());
    let text = all_text(&buf);
    // 80 / 17 is four columns, windowed around the selected Day 5.
    for shown in ["Day 3", "Day 4", "Day 5", "Day 6"] {
        assert!(text.contains(shown), "{shown} is missing at 80 columns");
    }
    for hidden in ["Day 0", "Day 1", "Day 2"] {
        assert!(!text.contains(hidden), "{hidden} was drawn at 80 columns");
    }
    // And the three that are not there are stated, spelled, on the bottom rule.
    assert!(
        row_text(&buf, 39).contains("three more days"),
        "{:?}",
        row_text(&buf, 39)
    );

    // Nothing is said when nothing is missing: 7 * 17 is 119.
    for width in [119u16, 120, 200] {
        let buf = drawn(width, 40, &workspace());
        assert!(
            all_text(&buf).contains("Day 0"),
            "a day is missing at {width}"
        );
        assert!(
            !row_text(&buf, 39).contains("more day"),
            "days are claimed missing at {width}: {:?}",
            row_text(&buf, 39)
        );
    }
    // One missing day is one day, not "one more days".
    let buf = drawn(118, 40, &workspace());
    let rule = row_text(&buf, 39);
    assert!(rule.contains("one more day"), "{rule:?}");
    assert!(!rule.contains("one more days"), "{rule:?}");
}

/// The window follows the selected day and stops at both ends of the week, so
/// `H`/`L` scroll it without it ever running off.
#[test]
fn the_window_follows_the_selection_and_clamps_at_the_ends() {
    let mut workspace = workspace();
    let windows = [
        (0usize, "Day 0", "Day 3"),
        (3, "Day 1", "Day 4"),
        (6, "Day 3", "Day 6"),
    ];
    for (selected, first, last) in windows {
        workspace.week.selected = selected;
        let buf = drawn(80, 40, &workspace);
        let text = all_text(&buf);
        assert!(
            text.contains(first),
            "{first} is missing with {selected} selected"
        );
        assert!(
            text.contains(last),
            "{last} is missing with {selected} selected"
        );
        // Exactly four columns, so the window never overhangs either end.
        let frames = (0..80u16).filter(|c| buf[(*c, 1)].symbol() == "┌").count();
        assert_eq!(frames, 4, "{frames} columns with {selected} selected");
        // And the selected day is always in the window.
        assert!(text.contains(&format!("Day {selected}")));
    }
}

/// The remainder of the division is spread a cell at a time, so no column is more
/// than one cell wider than another — and the row still reaches the right edge.
///
/// The last column used to take the whole remainder, which made it six cells wider
/// than its neighbours at 137 columns.
#[test]
fn no_column_is_more_than_one_cell_wider_than_another() {
    for width in [70u16, 80, 100, 120, 137, 200] {
        let mut workspace = workspace();
        workspace.week.selected = 0;
        let buf = drawn(width, 40, &workspace);
        let starts: Vec<u16> = (0..width)
            .filter(|c| buf[(*c, 1)].symbol() == "┌")
            .collect();
        assert!(!starts.is_empty(), "no columns at {width}");
        let mut widths: Vec<u16> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
        widths.push(width - starts[starts.len() - 1]);
        let narrowest = *widths.iter().min().expect("a column");
        let widest = *widths.iter().max().expect("a column");
        assert!(
            widest - narrowest <= 1,
            "columns differ by {} cells at {width}: {widths:?}",
            widest - narrowest
        );
        // The design's own stride survives at 120, where the remainder is one.
        if width == 120 {
            assert_eq!(starts, [0, 17, 34, 51, 68, 85, 102]);
        }
    }
}

/// `1b` brightens two columns out of seven — the selected day and today — and
/// leaves the other five in the fading step. Its column text divs are `var(--t)`
/// for Wed 26 and Thu 27 and `var(--t2)` for the rest.
#[test]
fn the_selected_day_and_today_are_brighter_than_the_rest() {
    let mut workspace = workspace();
    // No day is today until the workspace says which, so only the selection is
    // bright: a fallback here would brighten the last column of a week that has
    // already ended and call it today.
    let buf = drawn(120, 40, &workspace);
    let highlight = 4;
    assert_eq!(style_at(&buf, 1, highlight), Role::Fading.style(), "Day 0");
    assert_eq!(style_at(&buf, 86, highlight), Role::Body.style(), "Day 5");
    assert_eq!(
        style_at(&buf, 103, highlight),
        Role::Fading.style(),
        "Day 6"
    );

    // Day 6's `day_of_month` is "26"; naming it today brightens its column too.
    workspace.today.day_of_month = "26".to_owned();
    let buf = drawn(120, 40, &workspace);
    assert_eq!(style_at(&buf, 103, highlight), Role::Body.style(), "today");
    assert_eq!(style_at(&buf, 86, highlight), Role::Body.style(), "Day 5");
    assert_eq!(style_at(&buf, 1, highlight), Role::Fading.style(), "Day 0");
    // A hard entry keeps the caution colour whichever column it is in.
    let mut hard = workspace.clone();
    hard.week.days[0].highlights = vec![Entry::line("Incident").hard()];
    let buf = drawn(120, 40, &hard);
    assert_eq!(style_at(&buf, 1, highlight), Role::Caution.style());
}

/// A column too narrow for prose draws its frame and its date and stops, rather
/// than filling with words broken mid-letter.
#[test]
fn a_column_too_narrow_for_prose_draws_no_prose() {
    let workspace = workspace();
    // One column of eight cells: below `MIN_COLUMN_CELLS`.
    let buf = drawn(8, 40, &workspace);
    let text = all_text(&buf);
    assert!(
        !text.contains("Moved"),
        "prose was shredded into a sliver: {text}"
    );
    assert_eq!(buf[(0, 1)].symbol(), "┌", "the frame is missing");
    // Ten cells is enough, and then the column fills.
    let buf = drawn(10, 40, &workspace);
    assert!(
        all_text(&buf).contains("Moved"),
        "a wide-enough column is empty"
    );
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
