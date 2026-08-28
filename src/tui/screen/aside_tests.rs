//! The right-hand panels' own tests, in a sibling file.
//!
//! Split out of `aside.rs` to keep both inside the ~600-line soft target from
//! `README.md`, the same way `screen/tests.rs` and `cli/tests.rs` are separate
//! files. Nothing else moved: these are `aside.rs`'s tests, reached through a
//! `#[path]` module there, so `super::*` still names its private items.

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
        short_summary: None,
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

/// The one hard thing in a day's log takes the caution colour; the rest is body
/// text and a notable line brightens instead.
///
/// This is the only place `Tone::Hard` reaches [`Role::Caution`] in the Today
/// panel, and nothing covered it — `1a` draws the log in body text throughout,
/// so the mapping was live and untested. `1i` allows the colour twice a week,
/// which is exactly why it must be the tone that reaches for it and not a rank.
#[test]
fn a_hard_entry_is_the_one_yellow_line_in_the_log() {
    let mut day = a_day();
    day.entries = vec![
        Entry::at("08:10", "Rode in"),
        Entry::at("09:42", "The ring overflowed").hard(),
        Entry::at("11:52", "Finished the novel"),
    ];
    let week = vec![day.clone()];
    let buf = drawn(48, 16, |grid| {
        today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
    });
    for (needle, role) in [
        ("Rode in", Role::Body),
        ("The ring overflowed", Role::Caution),
        ("Finished the novel", Role::Body),
    ] {
        let row = find_row(&buf, needle);
        assert_eq!(
            style_at(&buf, col_of(&buf, row, needle), row),
            role.style(),
            "{needle} is not {role:?}"
        );
    }

    // And the third tone brightens rather than reaching for the caution colour.
    day.entries = vec![Entry {
        tone: Tone::Notable,
        ..Entry::at("11:52", "Finished the novel")
    }];
    let buf = drawn(48, 16, |grid| {
        today(grid, &day, &week, 0, false, Place::new(0, 0, 48, 16));
    });
    let row = find_row(&buf, "Finished the novel");
    assert_eq!(
        style_at(&buf, col_of(&buf, row, "Finished the novel"), row),
        Role::Strongest.style()
    );
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
        threads(grid, &list, false, 0, Place::new(0, 0, 48, 14))
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
        threads(grid, &list, false, 0, Place::new(0, 0, 48, 14))
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

/// The cursor brightens the row it is on without moving it, and only while
/// the panel holds focus — the Today screen's `J`/`K` used to move a
/// highlight nothing drew.
#[test]
fn the_cursor_brightens_a_focused_row_without_moving_it() {
    let list: Vec<Thread> = (0..3)
        .map(|n| thread(&format!("Thought {n}"), [true; 7], ""))
        .collect();
    let text_col = 1 + THREAD_TEXT;

    let focused = drawn(48, 14, |grid| {
        threads(grid, &list, true, 2, Place::new(0, 0, 48, 14))
    });
    // The rows do not move: the third thread is still third.
    assert_eq!(find_row(&focused, "Thought 2"), 3);
    assert_eq!(
        style_at(&focused, text_col, 3),
        Role::Strongest.style(),
        "the cursor row is not brightened"
    );
    // And the row above keeps its own step, so the cursor is one row.
    assert_eq!(style_at(&focused, text_col, 2), Role::Body.style());

    // Unfocused, nothing is brightened out of its rank.
    let idle = drawn(48, 14, |grid| {
        threads(grid, &list, false, 2, Place::new(0, 0, 48, 14))
    });
    assert_eq!(
        style_at(&idle, text_col, 3),
        Strength::from_rank(2).style(),
        "an unfocused panel drew a cursor"
    );
}

/// The thread panel's own margin, held by the two artboard lines that fixed it.
///
/// `1a` gives the panel 48 cells — 46 of interior, 35 past `THREAD_TEXT` — and
/// breaks `Every day this week · eight / other notes lean on it` after `eight`
/// and `The Quillstone cache lives on / the NAS at /srv/quillstone` after `on`.
/// Both fit one more word at the two-cell margin every other panel uses, so both
/// reflowed; `THREAD_MARGIN` is four because the artboard's breaks say four.
#[test]
fn the_thread_panels_margin_reproduces_the_artboards_breaks() {
    let list = vec![
        thread(
            "The ring holds 512 in flight; overflow blocks, never drops",
            [true; 7],
            "Every day this week · eight other notes lean on it",
        ),
        thread(
            "The Quillstone cache lives on the NAS at /srv/quillstone",
            [true, true, false, false, true, true, true],
            "",
        ),
    ];
    let buf = drawn(48, 14, |grid| {
        threads(grid, &list, false, 0, Place::new(0, 0, 48, 14))
    });
    let text = all_text(&buf);
    // Each pair is a line the artboard draws and the word that must not join it.
    for (line, spilled) in [
        ("The ring holds 512 in flight;", "flight; overflow"),
        ("overflow blocks, never drops", ""),
        ("Every day this week · eight", "eight other"),
        ("other notes lean on it", ""),
        ("The Quillstone cache lives on", "on the"),
        ("the NAS at /srv/quillstone", ""),
    ] {
        assert!(text.contains(line), "{line:?} is not a line: {text}");
        if !spilled.is_empty() {
            assert!(
                !text.contains(spilled),
                "the line ran on past the artboard's break: {spilled:?}"
            );
        }
    }
}

/// A thread whose reason is that it just came back is drawn in the returning
/// colour — the one thing blue means.
#[test]
fn a_returning_reason_is_blue() {
    let mut list = vec![thread("The 512 cap", [true; 7], "")];
    list[0].because = Justification::came_back("Came back just now");
    let buf = drawn(48, 14, |grid| {
        threads(grid, &list, false, 0, Place::new(0, 0, 48, 14))
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
    // Spelled and capitalised, as `1d` writes it: " Eight things lean on it:".
    assert!(text.contains("Two things lean on it:"), "{text}");
    assert!(text.contains("The oncall runbook"));
}

/// One dependent is "One thing leans on it:", not "One things lean on it:".
///
/// A single-element `leaned_on` is ordinary data — `today.rs`'s own fixture is
/// exactly that — so this is reached without anything unusual happening, and
/// English inflects the noun. `week_offscreen`/`week_offscreen_one` got the same
/// two-key treatment for the same reason.
#[test]
fn one_dependent_reads_as_one() {
    let mut one = thread("Block, never drop", [true; 7], "");
    one.leaned_on = vec!["The short postmortem".to_owned()];
    let buf = drawn(48, 14, |grid| {
        leans_on(grid, &one, Place::new(0, 0, 48, 14))
    });
    let text = all_text(&buf);
    assert!(text.contains("One thing leans on it:"), "{text}");
    assert!(!text.contains("things lean on it"), "{text}");
}

/// A dependent too long for the panel is ellipsised, not cut mid-word with no
/// mark at all.
///
/// The list used to go straight through `Grid::lines`, which clips at the panel
/// edge and says nothing about it: `1d`'s own "The 40ms fairness quantum assumes
/// writers wait" came out as "The 40ms fairness quantum assum" — a name the
/// reader has no reason to doubt, and not the name.
#[test]
fn a_long_dependent_says_it_was_cut() {
    let mut one = thread("Block, never drop", [true; 7], "");
    one.leaned_on = vec![
        "The 40ms fairness quantum assumes writers wait".to_owned(),
        "Short".to_owned(),
    ];
    let buf = drawn(30, 14, |grid| {
        leans_on(grid, &one, Place::new(0, 0, 30, 14))
    });
    let text = all_text(&buf);
    assert!(text.contains('…'), "the cut is unmarked: {text}");
    assert!(!text.contains("assumes writers wait"), "{text}");
    // And an entry that fits is left alone — no ellipsis on a whole name.
    let short = find_row(&buf, "Short");
    assert!(!row_text(&buf, short).contains('…'), "{text}");
}

/// The panel heads with the thread's short label where it has one, because `1d`
/// gives that head one row and spends the rest on the names below it.
#[test]
fn the_leans_panel_heads_with_the_short_label() {
    let mut one = thread(
        "The ring holds 512 in flight; overflow blocks, never drops",
        [true; 7],
        "",
    );
    one.short_summary = Some("Block, never drop".to_owned());
    one.leaned_on = vec!["The short postmortem".to_owned()];
    let buf = drawn(48, 14, |grid| {
        leans_on(grid, &one, Place::new(0, 0, 48, 14))
    });
    let text = all_text(&buf);
    assert!(text.contains("Block, never drop"), "{text}");
    assert!(
        !text.contains("overflow blocks"),
        "the long form is drawn too"
    );

    // A thread with no label heads with its full thought rather than nothing.
    one.short_summary = None;
    let buf = drawn(48, 14, |grid| {
        leans_on(grid, &one, Place::new(0, 0, 48, 14))
    });
    assert!(
        all_text(&buf).contains("The ring holds 512"),
        "no head was drawn"
    );
}

/// The list accounts for every name the header counts. It used to draw eight of
/// nine and say nine, with `Grid::lines` dropping the last at the interior edge
/// and nothing marking it.
#[test]
fn the_leans_list_accounts_for_its_own_count() {
    let mut one = thread("Block, never drop", [true; 7], "");
    one.leaned_on = (0..9).map(|n| format!("Dependent number {n}")).collect();
    // Twelve interior rows: a head, a blank, then ten rows for nine names.
    let buf = drawn(48, 8, |grid| leans_on(grid, &one, Place::new(0, 0, 48, 8)));
    let text = all_text(&buf);
    assert!(text.contains("Nine things lean on it:"), "{text}");
    assert!(
        text.contains("more"),
        "the missing names are not accounted for: {text}"
    );

    // A list that fits says nothing about a remainder.
    one.leaned_on = vec!["The short postmortem".to_owned()];
    let buf = drawn(48, 14, |grid| {
        leans_on(grid, &one, Place::new(0, 0, 48, 14))
    });
    let text = all_text(&buf);
    assert!(text.contains("One thing leans on it:"), "{text}");
    assert!(
        !text.contains("more"),
        "a list that fits claims a remainder: {text}"
    );
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

/// A cut trickle entry says it was cut. This panel is the only place these
/// lines appear anywhere in the app, so a clean word-boundary truncation reads
/// as a sentence the reader has no reason to doubt — and there is no keypress
/// that recovers the rest.
#[test]
fn a_cut_trickle_entry_is_marked_as_cut() {
    let list = vec![Trickle::new(
        "A very long thing that will not fit on one row of this narrow panel at all",
    )];
    let buf = drawn(48, 7, |grid| {
        trickle(grid, &list, false, Place::new(0, 0, 48, 7))
    });
    assert!(row_text(&buf, 1).contains('…'), "{:?}", row_text(&buf, 1));

    // An entry that fits is not marked.
    let list = vec![Trickle::new("Call Mum back")];
    let buf = drawn(48, 7, |grid| {
        trickle(grid, &list, false, Place::new(0, 0, 48, 7))
    });
    assert!(!row_text(&buf, 1).contains('…'), "{:?}", row_text(&buf, 1));
}

/// A trickle longer than its panel stops at the bottom rule rather than
/// writing over it.
///
/// A 7-row panel has 5 interior rows, so `Line 4` is the last that fits and
/// `Line 5` is the first dropped. Asserting on `Line 6` instead would have
/// passed with an off-by-one that wrote a row over the rule.
#[test]
fn an_overlong_trickle_stops_at_the_rule() {
    let list: Vec<Trickle> = (0..20)
        .map(|n| Trickle::new(&format!("Line {n}")))
        .collect();
    let buf = drawn(48, 7, |grid| {
        trickle(grid, &list, false, Place::new(0, 0, 48, 7))
    });
    let text = all_text(&buf);
    assert!(
        text.contains("Line 4"),
        "the last row that fits was dropped"
    );
    assert!(!text.contains("Line 5"), "a row was written past the panel");
    // The bottom rule is whole where the content would have reached it: the
    // trickle writes its bullet at column 2, not column 0.
    assert_eq!(row_text(&buf, 6), "└".to_owned() + &"─".repeat(46) + "┘");
}

/// One trickle entry is one row: a long line is clipped rather than pushing
/// the entries under it down.
#[test]
fn a_long_trickle_entry_takes_one_row() {
    let list = vec![
        Trickle::new("A very long thing that will not fit on one row of this narrow panel at all"),
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
        threads(grid, &list, true, 0, Place::new(0, 0, 48, 6))
    });
    assert_eq!(style_at(&buf, 0, 0), Role::Accent.style());
    let buf = drawn(48, 6, |grid| {
        threads(grid, &list, false, 0, Place::new(0, 0, 48, 6))
    });
    assert_eq!(style_at(&buf, 0, 0), Role::Furniture.style());
}
