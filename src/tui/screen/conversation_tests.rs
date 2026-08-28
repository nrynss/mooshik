//! The conversation panel's own tests, in a sibling file.
//!
//! Split out of `conversation.rs` to keep both inside the ~600-line soft target
//! from `README.md`, the same way `screen/tests.rs` and `cli/tests.rs` are
//! separate files. Nothing else moved: these are `conversation.rs`'s tests,
//! reached through a `#[path]` module there, so `super::*` still names its
//! private items.

use super::*;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use crate::tui::model::Composer;

fn conversation_of(turns: Vec<Turn>) -> Conversation {
    Conversation {
        earlier: None,
        turns,
        composer: Composer::default(),
    }
}

fn said(time: &str, speaker: Speaker, text: &str) -> Turn {
    Turn::Said {
        time: time.to_owned(),
        speaker,
        text: text.to_owned(),
    }
}

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

fn style_at(buf: &Buffer, col: u16, row: u16) -> Style {
    let cell = &buf[(col, row)];
    Style::default().fg(cell.fg).add_modifier(cell.modifier)
}

/// The row `needle` appears on. Tests locate content rather than assuming which
/// row it landed on, because how much of a long conversation is kept depends on
/// the panel's height.
fn find_row(buf: &Buffer, needle: &str) -> u16 {
    (0..buf.area.height)
        .find(|r| row_text(buf, *r).contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not on screen"))
}

fn text_of(spans: &[Span<'_>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The gutter and the text column, exactly as artboard `1a` places them:
/// the time from column 1 of the interior, the name from column 9.
#[test]
fn the_time_gutter_and_text_column_match_the_artboard() {
    let conversation = conversation_of(vec![said("09:04", Speaker::Person, "Postmortem's done.")]);
    let buf = drawn(72, 8, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 8));
    });
    // Interior column 1 is buffer column 2, whichever row the turn lands on —
    // the panel fills from the bottom.
    let speaker = find_row(&buf, "Neom");
    assert!(
        row_text(&buf, speaker).starts_with("│  09:04  Neom"),
        "{:?}",
        row_text(&buf, speaker)
    );
    assert!(row_text(&buf, speaker + 1).starts_with("│         Postmortem's done."));
}

/// A short or missing time never shifts the name out of its column.
#[test]
fn an_odd_time_does_not_move_the_name() {
    for time in ["", "9:04", "09:04", "09:04:33"] {
        let conversation = conversation_of(vec![said(time, Speaker::Person, "x")]);
        let buf = drawn(72, 6, |grid| {
            panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 6));
        });
        let row = row_text(&buf, find_row(&buf, "Neom"));
        assert_eq!(
            row.chars().nth(10),
            Some('N'),
            "time {time:?} moved the name: {row:?}"
        );
    }
}

/// The person's words are the brightest thing in the panel; Mooshik's are
/// body text under an accent name.
#[test]
fn the_two_speakers_are_coloured_apart() {
    let conversation = conversation_of(vec![
        said("09:04", Speaker::Person, "Mine"),
        said("09:05", Speaker::Mooshik, "Theirs"),
    ]);
    let buf = drawn(72, 10, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 10));
    });
    let mine = find_row(&buf, "Mine") - 1;
    assert_eq!(style_at(&buf, 10, mine), Role::Strongest.style());
    assert_eq!(style_at(&buf, 10, mine + 1), Role::Strongest.style());
    // A blank row, then Mooshik's turn: an accent name over body text.
    let theirs = find_row(&buf, "Theirs") - 1;
    assert_eq!(theirs, mine + 3, "the gap between turns is missing");
    assert_eq!(style_at(&buf, 10, theirs), Role::Accent.style());
    assert_eq!(style_at(&buf, 10, theirs + 1), Role::Body.style());
}

/// Whole turns are dropped from the front, never half a turn — a quotation
/// must never lose the attribution that makes it trustworthy.
#[test]
fn trimming_drops_whole_turns() {
    let turns: Vec<Turn> = (0..10)
        .map(|n| said("09:04", Speaker::Person, &format!("Line number {n}")))
        .collect();
    let conversation = conversation_of(turns);
    let buf = drawn(72, 8, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 8));
    });
    // Every speaker row drawn has its words on the row below it, so no turn
    // was cut in half.
    for row in 1..7u16 {
        if row_text(&buf, row).contains("Neom") {
            assert!(
                row_text(&buf, row + 1).contains("Line number"),
                "a turn lost its words at row {row}"
            );
        }
    }
    // The newest turn survives; the oldest does not.
    let all: String = (0..8).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains("Line number 9"));
    assert!(!all.contains("Line number 0"));
}

/// The elision marker takes the top of the panel and the turns start below
/// the blank row under it.
#[test]
fn the_elision_marker_sits_above_the_turns() {
    let mut conversation = conversation_of(vec![said("14:20", Speaker::Person, "Right.")]);
    conversation.earlier = Some("... earlier today".to_owned());
    let buf = drawn(72, 8, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 8));
    });
    // The marker takes the panel's first interior row whatever the turns do.
    assert!(row_text(&buf, 1).contains("... earlier today"));
    assert_eq!(style_at(&buf, 10, 1), Role::Furniture.style());
    let interior: String = row_text(&buf, 2).chars().skip(1).take(70).collect();
    assert!(interior.trim().is_empty(), "{interior:?}");
    // And the turn is below it, not above.
    assert!(find_row(&buf, "Neom") > 1);
}

/// The newest turn survives the trim even though the panel fills from the top —
/// whole turns are dropped from the *front*, so what is kept is the tail.
#[test]
fn the_newest_turn_survives_the_trim() {
    // Nine short turns into a panel that cannot hold them all.
    let turns: Vec<Turn> = (0..9)
        .map(|n| said("09:04", Speaker::Person, &format!("Line {n}")))
        .collect();
    let conversation = conversation_of(turns);
    let buf = drawn(72, 12, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 12));
    });
    let all: String = (0..12).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains("Line 8"), "the newest turn was dropped");
    assert!(all.contains("Line 6"), "too little of the tail was kept");
    assert!(!all.contains("Line 0"), "the oldest turn survived");
}

/// The panel is top-anchored, exactly as `1a` draws it: the elision marker on the
/// first interior row, one blank under it, the first turn on the row after that,
/// and the slack left at the bottom.
///
/// The artboard's conversation is a 33-row panel whose interior runs rows 2 to 32;
/// the marker is on row 2, the first speaker on row 4, and the last line of the
/// last turn on row 30 — two spare rows above the composer. Bottom-anchoring
/// closed that gap, which looked tidier and was not the design: the conversation
/// grows downwards into it. This used to be `slack_opens_up_under_the_elision
/// _marker`, which pinned the anchoring the other way round.
#[test]
fn the_panel_is_top_anchored_as_the_artboard_draws_it() {
    let turns: Vec<Turn> = (0..3)
        .map(|n| said("09:04", Speaker::Person, &format!("Line {n}")))
        .collect();
    let mut conversation = conversation_of(turns);
    conversation.earlier = Some("... earlier today".to_owned());
    let buf = drawn(72, 14, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 14));
    });
    // Buffer row 1 is the interior's first: the marker.
    assert!(row_text(&buf, 1).contains("... earlier today"));
    // Then a blank, then the turns from row 3 — the artboard's rows 2, 3, 4.
    let blank: String = row_text(&buf, 2).chars().skip(1).take(70).collect();
    assert!(blank.trim().is_empty(), "{blank:?}");
    assert!(
        row_text(&buf, 3).contains("Neom"),
        "{:?}",
        row_text(&buf, 3)
    );
    assert!(row_text(&buf, 4).contains("Line 0"));
    // Three turns of two rows with a gap between them end on row 10, and the
    // slack is the two rows under that rather than a gap above the composer.
    assert!(
        row_text(&buf, 10).contains("Line 2"),
        "{:?}",
        row_text(&buf, 10)
    );
    for row in [11u16, 12] {
        let slack: String = row_text(&buf, row).chars().skip(1).take(70).collect();
        assert!(slack.trim().is_empty(), "row {row} is not slack: {slack:?}");
    }
    assert_eq!(buf[(0, 13)].symbol(), "└");
}

/// Without a marker the first turn takes the panel's own first interior row.
#[test]
fn a_panel_with_no_marker_starts_on_its_first_row() {
    let conversation = conversation_of(vec![said("09:04", Speaker::Person, "Only turn")]);
    let buf = drawn(72, 12, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 12));
    });
    assert!(
        row_text(&buf, 1).contains("Neom"),
        "{:?}",
        row_text(&buf, 1)
    );
    assert!(row_text(&buf, 2).contains("Only turn"));
}

/// A conversation that fits keeps every turn, and says nothing about history it
/// did not have to drop.
///
/// The doc here used to claim the panel was "filled from the bottom", which was
/// the bottom-anchored behaviour reverted in round two, and the body asserted
/// nothing about anchoring — it repeated the setup of the test above it. What is
/// worth pinning is the other half of [`fit`]: it drops from the front only when
/// it has to, so a short day is drawn whole with no elision marker invented for
/// it.
#[test]
fn a_conversation_that_fits_keeps_every_turn() {
    let turns: Vec<Turn> = (0..4)
        .map(|n| said(&format!("09:0{n}"), Speaker::Person, &format!("Turn {n}")))
        .collect();
    let conversation = conversation_of(turns);
    let buf = drawn(72, 14, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 14));
    });
    let all: String = (0..14).map(|r| row_text(&buf, r)).collect();
    for n in 0..4 {
        assert!(all.contains(&format!("Turn {n}")), "turn {n} was dropped");
    }
    // And no marker: the fixture set none, and the panel does not invent one.
    assert!(!all.contains("earlier today"), "{all}");
}

/// A recall card is framed in the returning colour with its reason punched
/// through the bottom rule — and it is inline in the scroll, not an overlay.
#[test]
fn a_recall_card_is_framed_and_inline() {
    let conversation = conversation_of(vec![Turn::Recalled(Recall {
        source: "From Monday 24 August".to_owned(),
        quote: "Blocking the writer is honest.".to_owned(),
        because: "You've come back to this every day this week".to_owned(),
    })]);
    let buf = drawn(72, 8, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 8));
    });
    let top = find_row(&buf, "From Monday 24 August");
    // The card sits at the text indent, so its frame starts at interior
    // column 9 — buffer column 10.
    assert_eq!(buf[(10, top)].symbol(), "┌");
    assert_eq!(style_at(&buf, 10, top), Role::Returned.style());
    assert!(row_text(&buf, top + 2).contains("come back to this every day"));
}

/// A caution is a yellow frame in the conversation, with its reassurance on
/// the bottom rule and what leans on it listed inside.
#[test]
fn a_caution_is_a_yellow_frame_in_the_scroll() {
    let conversation = conversation_of(vec![Turn::Cautioned(Caution {
        lead: "You've held to \"block, never drop\" every day this week.".to_owned(),
        leaning: vec![
            "The short postmortem".to_owned(),
            "... and five more".to_owned(),
        ],
        because: "Nothing's changed — say the word and I'll follow".to_owned(),
    })]);
    let buf = drawn(72, 14, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 14));
    });
    // The caution's name is chrome, not content: it comes from `en.toml`, and the
    // model no longer carries a title at all.
    let title = crate::text::get("tui.panel_caution");
    assert_eq!(title, "One thing before you do");
    // The caution sits two columns left of the text indent — buffer 8.
    let top = find_row(&buf, title);
    assert_eq!(buf[(8, top)].symbol(), "┌");
    assert_eq!(style_at(&buf, 8, top), Role::Caution.style());
    let all: String = (0..14).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains(title));
    assert!(all.contains("The short postmortem"));
    assert!(all.contains("say the word and I'll follow"));
}

/// A dependency too long for the card is ellipsised, not cut mid-word with no
/// mark at all.
///
/// The list went straight through `Grid::lines`, which clips at the card's edge
/// and says nothing about it: at 45 to 60 columns `1d`'s own "The 40ms fairness
/// quantum assumes writers wait" came out as "The 40ms fairness quantum assum",
/// which reads as a name rather than as a truncation.
#[test]
fn a_long_dependency_in_a_caution_says_it_was_cut() {
    let conversation = conversation_of(vec![Turn::Cautioned(Caution {
        lead: "You've held to it.".to_owned(),
        leaning: vec![
            "The 40ms fairness quantum assumes writers wait".to_owned(),
            "Short".to_owned(),
        ],
        because: "Nothing's changed".to_owned(),
    })]);
    let buf = drawn(45, 14, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 45, 14));
    });
    let all: String = (0..14).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains('…'), "the cut is unmarked: {all}");
    assert!(!all.contains("assumes writers wait"), "{all}");
    // A name that fits keeps its whole self and gains no mark.
    let short = find_row(&buf, "Short");
    assert!(!row_text(&buf, short).contains('…'), "{all}");
}

/// The quoted commitment inside a caution is brightened, and nothing is
/// dropped in the process.
#[test]
fn a_quotation_is_emphasised_without_losing_characters() {
    let line = "You've held to \"block, never drop\" every day";
    let spans = emphasise_quoted(line, &mut false);
    assert_eq!(text_of(&spans), line);
    let quoted: String = spans
        .iter()
        .filter(|s| s.style == Role::Strongest.style())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(quoted, "\"block, never drop\"");
}

/// An unclosed quotation mark emphasises the remainder rather than dropping
/// it or panicking.
#[test]
fn an_unclosed_quotation_keeps_every_character() {
    for line in [
        "no quotes here",
        "\"opens only",
        "closes only\"",
        "\"\"",
        "\"a\"b\"c",
    ] {
        let spans = emphasise_quoted(line, &mut false);
        assert_eq!(text_of(&spans), line, "characters lost in {line:?}");
    }
}

/// The composer shows a prompt and a cursor with an empty draft, and both
/// with a draft — and the reassurance takes its own row when there is one.
#[test]
fn the_composer_draws_a_cursor_with_or_without_a_draft() {
    let empty = conversation_of(Vec::new());
    let buf = drawn(72, 4, |grid| {
        composer(grid, &empty, false, Place::new(0, 0, 72, 4))
    });
    assert!(
        row_text(&buf, 1).starts_with("│ ▌█"),
        "{:?}",
        row_text(&buf, 1)
    );
    assert!(row_text(&buf, 2).contains("Nothing here needs saving"));

    let mut typed = conversation_of(Vec::new());
    typed.composer.draft = "Called Mum.".to_owned();
    let buf = drawn(72, 4, |grid| {
        composer(grid, &typed, false, Place::new(0, 0, 72, 4))
    });
    assert!(
        row_text(&buf, 1).starts_with("│ ▌ Called Mum.█"),
        "{:?}",
        row_text(&buf, 1)
    );
}

/// A three-row composer has no room for the reassurance and simply omits it
/// rather than writing over its own rule.
#[test]
fn a_narrow_composer_omits_the_reassurance() {
    let empty = conversation_of(Vec::new());
    let buf = drawn(80, 3, |grid| {
        composer(grid, &empty, false, Place::new(0, 0, 80, 3))
    });
    let all: String = (0..3).map(|r| row_text(&buf, r)).collect();
    assert!(!all.contains("Nothing here needs saving"));
    assert!(all.contains("▌█"));
}

/// A panel with no interior draws nothing and does not panic.
///
/// "Nothing" means no *content*: a 2x2 frame is all rule and has no interior, so
/// every cell must be a box-drawing character and none of the turn's words may
/// appear. The old version of this test asserted `buf.area.width == 4`, which
/// `Buffer::empty` guarantees, so it could not have failed.
#[test]
fn a_panel_with_no_interior_draws_nothing() {
    let conversation = conversation_of(vec![said("09:04", Speaker::Person, "x")]);
    let buf = drawn(4, 2, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 2, 2));
        composer(grid, &conversation, false, Place::new(2, 0, 2, 2));
    });
    let all: String = (0..2).map(|r| row_text(&buf, r)).collect();
    assert!(
        all.chars().all(|c| "┌┐└┘─│".contains(c)),
        "something was drawn inside a frame with no interior: {all:?}"
    );
    assert!(!all.contains("Neom"));
    assert!(!all.contains("09:04"));
}

/// A turn taller than the whole panel is kept and clipped, not dropped.
///
/// Dropping it left `kept` empty and the panel showing nothing but its frame,
/// with no key in the app that scrolls it back. The tail survives — the newest
/// words — under the speaker's own row, which is never trimmed.
#[test]
fn a_turn_taller_than_the_panel_is_clipped_not_dropped() {
    // Thirty-two wrapped lines of numbered words, into a 6-row interior.
    let long: String = (0..190).map(|n| format!("word{n} ")).collect();
    let conversation = conversation_of(vec![said("09:04", Speaker::Mooshik, long.trim())]);
    let buf = drawn(72, 8, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 8));
    });
    let all: String = (0..8).map(|r| row_text(&buf, r)).collect();
    assert!(
        all.contains("Mooshik"),
        "the speaker's row was trimmed away: {all:?}"
    );
    // The tail is what survives, so the last word is there and the first is not.
    assert!(all.contains("word189"), "the tail was dropped: {all:?}");
    assert!(!all.contains("word0 "), "the head survived: {all:?}");
    // And nothing was written over the panel's own rules.
    assert_eq!(buf[(0, 0)].symbol(), "┌");
    assert_eq!(buf[(0, 7)].symbol(), "└");
}

/// A card is drawn no wider than the panel holding it.
///
/// The artboards' `w=54` and `w=58` are the widths at 120 columns and were used
/// unconditionally, so on a narrow terminal the frame's right edge fell off the
/// grid and the quote clipped mid-word with no ellipsis.
#[test]
fn a_card_narrows_with_its_panel() {
    let conversation = conversation_of(vec![Turn::Recalled(Recall {
        source: "From Monday".to_owned(),
        quote: "Blocking the writer is honest, dropping it is a lie.".to_owned(),
        because: "Every day this week".to_owned(),
    })]);
    for width in [30u16, 40, 60, 72] {
        let buf = drawn(width, 10, |grid| {
            panel(
                grid,
                "Neom",
                &conversation,
                false,
                Place::new(0, 0, width, 10),
            );
        });
        let top = find_row(&buf, "From Monday");
        let row = row_text(&buf, top);
        // The card's frame opens at the text indent and must close inside the
        // panel's own right rule.
        assert_eq!(buf[(10, top)].symbol(), "┌", "at {width}: {row:?}");
        let closes = row
            .char_indices()
            .filter(|(_, c)| *c == '┐')
            .map(|(i, _)| row[..i].chars().count())
            .collect::<Vec<_>>();
        assert!(
            closes.iter().any(|c| *c < usize::from(width) - 1),
            "the card's right edge fell off the grid at {width}: {row:?}"
        );
        // And every wrapped line of the quote stays inside the card.
        for r in top..top + 3 {
            assert_eq!(
                row_text(&buf, r).chars().count(),
                usize::from(width),
                "row {r} is the wrong width at {width}"
            );
        }
    }
}

/// A quotation that spans a wrapped line keeps its emphasis on both sides of the
/// break.
///
/// `emphasise_quoted` used to derive inside-or-outside from `index % 2` within one
/// line, and it is applied per line, so the state restarted at "outside" on every
/// one. At 40 columns `1d`'s opening breaks inside the quotation and the second
/// line came out exactly inverted: `drop` in body text, and the words after the
/// closing mark in the brightest step.
#[test]
fn a_quotation_across_a_wrap_is_not_inverted() {
    let lead = "You've held to \"block, never drop\" every day this week";
    let lines = crate::tui::wrap::wrap(lead, 28);
    assert!(lines.len() > 1, "the fixture does not wrap: {lines:?}");
    assert!(
        lines[0].contains("never") && lines[1].starts_with("drop\""),
        "the wrap does not land inside the quotation: {lines:?}"
    );

    let mut inside = false;
    let first = emphasise_quoted(&lines[0], &mut inside);
    assert!(inside, "the state did not survive the first line");
    let second = emphasise_quoted(&lines[1], &mut inside);
    assert!(!inside, "the closing mark did not end the quotation");

    // Everything up to the opening mark is body text; everything after it, on
    // either line, is the brightest step until the closing mark.
    let bright = |spans: &[Span<'static>]| -> String {
        spans
            .iter()
            .filter(|s| s.style == Role::Strongest.style())
            .map(|s| s.content.as_ref().to_owned())
            .collect()
    };
    assert_eq!(bright(&first), "\"block, never");
    assert_eq!(bright(&second), "drop\"");
    // And the tail of the second line is back to body text.
    assert!(
        second
            .iter()
            .any(|s| s.style == Role::Body.style() && s.content.contains("every day")),
        "the words after the quotation are not body text"
    );
    // Nothing was lost either way.
    assert_eq!(text_of(&first), lines[0]);
    assert_eq!(text_of(&second), lines[1]);
}

/// A caution's body breaks where `1d` breaks it, and no card body line reaches
/// its own frame.
///
/// Nothing pinned this dimension, which is how both card bodies came to be
/// wrapped with no right margin at all: `CARD_CHROME` covers the two rules and
/// the inset, and subtracting it alone left the text ending on the cell beside
/// the rule. `1d` is the artboard whose whole content is this card.
#[test]
fn a_cards_body_breaks_where_the_artboard_breaks_it() {
    let caution = Caution {
        lead: "You've held to \"block, never drop\" every day this week — it's the thing \
               you come back to most, and eight other notes lean on it:"
            .to_owned(),
        leaning: vec!["The postmortem is short because nothing dropped".to_owned()],
        because: "Nothing's changed — say the word and I'll follow".to_owned(),
    };
    let conversation = conversation_of(vec![Turn::Cautioned(caution)]);
    let buf = drawn(72, 20, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 20));
    });

    // `1d`'s own three breaks, at the design's own card width.
    let text: String = (0..20u16)
        .map(|row| row_text(&buf, row))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("You've held to \"block, never drop\" every day this"),
        "the first line is not the artboard's:\n{text}"
    );
    assert!(
        text.contains("week — it's the thing you come back to most, and"),
        "the second line is not the artboard's:\n{text}"
    );
    assert!(
        text.contains("eight other notes lean on it:"),
        "the third line is not the artboard's:\n{text}"
    );

    // And no body row ends on the cell beside the card's own rule. The caution
    // sits at interior column 7 and is 58 wide, so on the buffer its left rule is
    // column 8 and its right rule column 65 — the body's own rows are the ones
    // with that left rule, and column 64 on them must be clear.
    let mut body_rows = 0;
    for row in 0..20u16 {
        let line = row_text(&buf, row);
        let mut cells = line.chars();
        if cells.nth(8) != Some('│') {
            continue;
        }
        body_rows += 1;
        assert_eq!(
            line.chars().nth(64),
            Some(' '),
            "row {row} puts text against the card's rule: {line:?}"
        );
    }
    assert!(body_rows >= 3, "the card drew no body to check");
}

/// A recall card's quote breaks where `1c` breaks it, and its frame sits where
/// the artboard puts it.
///
/// The sibling of the caution test above, and it exists for the same reason that
/// one does: nothing pinned `1c`'s geometry, so `RECALL`'s width could move four
/// columns and lose the artboard's first break with all 496 tests still green.
/// `1d` was closed a round earlier; this is the other half.
#[test]
fn a_recall_cards_quote_breaks_where_the_artboard_breaks_it() {
    let conversation = conversation_of(vec![Turn::Recalled(Recall {
        source: "From Monday 24 August".to_owned(),
        quote: "Blocking the writer is honest. Dropping is a lie the consumer only \
                finds out an hour later."
            .to_owned(),
        because: "You've come back to this every day this week".to_owned(),
    })]);
    let buf = drawn(72, 12, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 12));
    });
    let text: String = (0..12u16)
        .map(|row| row_text(&buf, row))
        .collect::<Vec<_>>()
        .join("\n");

    // `1c`'s own two breaks.
    assert!(
        text.contains("Blocking the writer is honest. Dropping is a lie"),
        "the first line is not the artboard's:\n{text}"
    );
    assert!(
        text.contains("the consumer only finds out an hour later."),
        "the second line is not the artboard's:\n{text}"
    );

    // The card sits at the conversation's text indent and is 54 wide, so on the
    // buffer its left rule is column 10 and its right rule column 63.
    //
    // The clear cell inside that rule is checked but is *not* what pins
    // `RIGHT_MARGIN`: `1c`'s longest line is 48 characters and its next word
    // takes it to 52, so both breaks and the clear cell survive even without the
    // margin. The caution test above is the one that holds it — its lines run to
    // 54. This assertion catches a frame that moved, not a margin that went.
    let mut body_rows = 0;
    for row in 0..12u16 {
        let line = row_text(&buf, row);
        if line.chars().nth(10) != Some('│') {
            continue;
        }
        body_rows += 1;
        assert_eq!(line.chars().nth(63), Some('│'), "row {row}: {line:?}");
        assert_eq!(
            line.chars().nth(62),
            Some(' '),
            "row {row} puts text against the card's rule: {line:?}"
        );
    }
    assert!(body_rows >= 2, "the card drew no body to check");
}

/// A card is clipped from its *tail*, so the statement still starts at its first
/// word — where a turn is clipped from its front, so the newest words survive.
#[test]
fn a_card_is_clipped_from_the_tail_and_a_turn_from_the_front() {
    let lead: Vec<String> = (0..6).map(|n| format!("Lead line {n}")).collect();
    let card = Caution {
        lead: lead.join(" "),
        leaning: vec!["Leans".to_owned()],
        because: "Nothing's changed".to_owned(),
    };
    let block = Block::Cautioned {
        card,
        lead: lead.clone(),
    };
    let Some(Block::Cautioned { card, lead: kept }) = block.clip_to(7) else {
        panic!("clip_to declined a block it had room for")
    };
    // The list is trimmed before the lead — but not all the way to nothing. The
    // lead's last line is the colon that introduces the list, so a card that
    // showed no names announced one and then did not: the lead gives up that
    // line so the colon is true.
    assert_eq!(kept, ["Lead line 0", "Lead line 1"]);
    assert_eq!(card.leaning, ["Leans"], "the colon points at nothing");

    // And the trimming order itself is unchanged: a list with room to spare is
    // still what gives way first.
    let one_line = vec!["Only line".to_owned()];
    let block = Block::Cautioned {
        card: Caution {
            lead: one_line.join(" "),
            leaning: (0..5).map(|n| format!("Leans {n}")).collect(),
            because: "Nothing's changed".to_owned(),
        },
        lead: one_line.clone(),
    };
    let Some(Block::Cautioned { card, lead: kept }) = block.clip_to(7) else {
        panic!("clip_to declined a block it had room for")
    };
    assert_eq!(
        kept, one_line,
        "the lead was trimmed while the list had room"
    );
    assert_eq!(card.leaning.len(), 2, "the list keeps what the lead left");

    // A turn, the other way round.
    let block = Block::Said {
        time: "09:04".to_owned(),
        name: "Neom".to_owned(),
        name_role: Role::Strongest,
        text_role: Role::Strongest,
        lines: lead.clone(),
    };
    let Some(Block::Said { lines, .. }) = block.clip_to(4) else {
        panic!("clip_to declined a block it had room for")
    };
    assert_eq!(lines, ["Lead line 3", "Lead line 4", "Lead line 5"]);
}

/// A card that still does not fit once clipped is dropped, not half-drawn.
///
/// `clip_to` cannot shrink a card below its own chrome — a recall clipped to one
/// row still measures two — and `fit` used to push it anyway, so the grid clipped
/// away the card's bottom rule and its badge. A one-row interior is reachable at
/// 120x9 on the wide layout and 80x11 on the narrow one.
#[test]
fn a_card_too_tall_for_one_row_is_dropped_rather_than_half_drawn() {
    let conversation = conversation_of(vec![Turn::Recalled(Recall {
        source: "From Monday".to_owned(),
        quote: "Blocking is honest.".to_owned(),
        because: "Every day this week".to_owned(),
    })]);
    // A three-row panel has one interior row; a recall card needs two.
    let buf = drawn(72, 3, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 3));
    });
    let all: String = (0..3).map(|r| row_text(&buf, r)).collect();
    assert!(!all.contains("From Monday"), "a half-framed card was drawn");
    assert!(!all.contains("Blocking is honest"));
    // The panel's own frame is whole, top and bottom.
    assert_eq!(buf[(0, 0)].symbol(), "┌");
    assert_eq!(buf[(0, 2)].symbol(), "└");
    assert_eq!(row_text(&buf, 1).trim_end().chars().last(), Some('│'));

    // Two interior rows is not enough either: a frame with no interior has no
    // title, no quote and no reason left in it.
    let buf = drawn(72, 4, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 4));
    });
    let all: String = (0..4).map(|r| row_text(&buf, r)).collect();
    assert!(
        !all.contains("From Monday"),
        "a frameful of nothing was drawn"
    );

    // Three is, and then it is drawn whole — rules, title, quote and badge.
    let buf = drawn(72, 5, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 5));
    });
    let all: String = (0..5).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains("From Monday"), "the card fits and was dropped");
    assert!(all.contains("Blocking is honest"));
    assert!(all.contains("Every day this week"));
}

/// The same for a caution, which needs four rows of chrome before a word of it
/// is legible.
#[test]
fn a_caution_too_tall_for_its_chrome_is_dropped() {
    let conversation = conversation_of(vec![Turn::Cautioned(Caution {
        lead: "You've held to \"block, never drop\".".to_owned(),
        leaning: vec!["The short postmortem".to_owned()],
        because: "Nothing's changed".to_owned(),
    })]);
    for height in [3u16, 4, 5] {
        let buf = drawn(72, height, |grid| {
            panel(
                grid,
                "Neom",
                &conversation,
                false,
                Place::new(0, 0, 72, height),
            );
        });
        let all: String = (0..height).map(|r| row_text(&buf, r)).collect();
        assert!(
            !all.contains("block, never drop"),
            "a broken caution was drawn at height {height}: {all:?}"
        );
        assert_eq!(buf[(0, height - 1)].symbol(), "└");
    }
}

/// The composer's rule carries the artboard's reassurance and nothing else.
///
/// It used to open with `Enter send  ·  Alt-Enter newline  ·  `. `Alt-Enter` put
/// a `'\n'` in the draft that `Buffer::set_stringn` filters out, into a composer
/// with one interior text row; `Enter` maps to a deliberate no-op. Both are
/// promises the app cannot keep yet, and `chrome`'s own rule is that a hint doing
/// nothing is worse than no hint.
#[test]
fn the_composer_rule_promises_no_key() {
    let empty = conversation_of(Vec::new());
    let buf = drawn(72, 4, |grid| {
        composer(grid, &empty, false, Place::new(0, 0, 72, 4))
    });
    let rule = row_text(&buf, 2);
    assert!(rule.contains("Nothing here needs saving"), "{rule:?}");
    assert!(!rule.contains("Enter"), "{rule:?}");
    assert!(!rule.contains("Alt"), "{rule:?}");
}
