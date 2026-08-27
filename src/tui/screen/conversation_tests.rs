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

/// The row `needle` appears on. The panel fills from the bottom, so tests
/// locate content rather than assuming which row it landed on.
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

/// The newest turn sits on the panel's bottom rule, so there is never a gap
/// between the last thing said and the composer under it.
#[test]
fn the_newest_turn_sits_on_the_bottom_rule() {
    // Nine short turns into a panel that cannot hold them all: whole turns
    // are dropped, so the kept ones will not fill it exactly.
    let turns: Vec<Turn> = (0..9)
        .map(|n| said("09:04", Speaker::Person, &format!("Line {n}")))
        .collect();
    let conversation = conversation_of(turns);
    let buf = drawn(72, 12, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 12));
    });
    // Interior row 9 is the last one; buffer row 10 sits on it.
    assert!(
        row_text(&buf, 10).contains("Line 8"),
        "the newest turn is not on the bottom rule: {:?}",
        row_text(&buf, 10)
    );
}

/// With an elision marker, the slack opens up under it rather than at the
/// bottom — the marker stays on the top row either way.
#[test]
fn slack_opens_up_under_the_elision_marker() {
    let turns: Vec<Turn> = (0..9)
        .map(|n| said("09:04", Speaker::Person, &format!("Line {n}")))
        .collect();
    let mut conversation = conversation_of(turns);
    conversation.earlier = Some("... earlier today".to_owned());
    let buf = drawn(72, 12, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 12));
    });
    assert!(row_text(&buf, 1).contains("... earlier today"));
    assert!(row_text(&buf, 10).contains("Line 8"));
}

/// A conversation that fits leaves the panel filled from the bottom without
/// pushing anything off the top.
#[test]
fn a_short_conversation_is_not_clipped() {
    let conversation = conversation_of(vec![said("09:04", Speaker::Person, "Only turn")]);
    let buf = drawn(72, 12, |grid| {
        panel(grid, "Neom", &conversation, false, Place::new(0, 0, 72, 12));
    });
    let all: String = (0..12).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains("Only turn"));
    assert!(all.contains("Neom"));
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
        title: "One thing before you do".to_owned(),
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
    // The caution sits two columns left of the text indent — buffer 8.
    let top = find_row(&buf, "One thing before you do");
    assert_eq!(buf[(8, top)].symbol(), "┌");
    assert_eq!(style_at(&buf, 8, top), Role::Caution.style());
    let all: String = (0..14).map(|r| row_text(&buf, r)).collect();
    assert!(all.contains("One thing before you do"));
    assert!(all.contains("The short postmortem"));
    assert!(all.contains("say the word and I'll follow"));
}

/// The quoted commitment inside a caution is brightened, and nothing is
/// dropped in the process.
#[test]
fn a_quotation_is_emphasised_without_losing_characters() {
    let line = "You've held to \"block, never drop\" every day";
    let spans = emphasise_quoted(line);
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
        let spans = emphasise_quoted(line);
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
