//! The conversation panel and the composer under it.
//!
//! One panel serves three artboards, because the design deliberately made them
//! one screen: `1a` is the conversation, `1c` puts a thing the user said on
//! another day inline where it was needed, and `1d` puts one careful sentence
//! where a reply would go. Neither `1c` nor `1d` is a modal, an overlay or a
//! separate mode — they are [`Turn`] variants in the same scroll, which is the
//! whole argument the design is making about how a caution should feel.
//!
//! **Why whole turns are dropped rather than lines.** The panel shows the tail
//! that fits and marks the rest with "... earlier today". Trimming line by line
//! would leave a turn without its speaker, or the bottom half of a quotation
//! with no attribution — the recall card would lose the very thing that makes it
//! trustworthy. So [`fit`] drops complete turns from the front, which is also
//! what the artboard shows.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Caution, Conversation, Recall, Speaker, Turn},
        theme::Role,
        widget::{Kind, Panel},
        wrap::wrap,
    },
};

/// The column continuation lines and speaker names sit at, inside the panel's
/// interior. The design's `--cw * 10` against a panel at column 0.
const INDENT: u16 = 9;
/// The column the timestamp gutter starts at.
const GUTTER: u16 = 1;

/// Where a recall card sits and how wide it is, inside the panel's interior.
/// The design's `col=10 w=54` against a panel at column 0.
const RECALL: (u16, u16) = (INDENT, 54);
/// The same for a caution, which sits two columns further left and four wider —
/// it is the more serious of the two and reads that way.
const CAUTION: (u16, u16) = (INDENT - 2, 58);
/// Indent of the "what leans on this" list inside a caution card.
const LEANING_INDENT: u16 = 3;
/// Columns a card's frame and inset take off its text width.
const CARD_CHROME: u16 = 3;
/// Rows the elision marker takes: itself, and the blank row under it.
const ELISION_ROWS: u16 = 2;

/// One thing in the conversation, measured and pre-wrapped for a given width.
///
/// Blocks are atomic: a block is either drawn whole or dropped whole. See this
/// module's header for why.
enum Block {
    /// A blank row between turns.
    Gap,
    /// Somebody spoke.
    Said {
        time: String,
        name: String,
        name_role: Role,
        text_role: Role,
        lines: Vec<String>,
    },
    /// A quotation from another day, in its own frame.
    Recalled { card: Recall, quote: Vec<String> },
    /// One careful sentence before the user changes their mind.
    Cautioned { card: Caution, lead: Vec<String> },
}

impl Block {
    /// How many rows this block occupies.
    fn height(&self) -> u16 {
        match self {
            Self::Gap => 1,
            // The speaker's row, then the words.
            Self::Said { lines, .. } => 1 + rows(lines.len()),
            // Two rules, and the quote between them.
            Self::Recalled { quote, .. } => 2 + rows(quote.len()),
            // Two rules, the lead, a blank, the list, and a blank before the
            // badge — the shape artboard `1d` draws.
            Self::Cautioned { card, lead } => {
                2 + rows(lead.len()) + 1 + rows(card.leaning.len()) + 1
            }
        }
    }

    /// Draw this block with its first row at `row` of `grid`.
    fn draw(&self, grid: &mut Grid<'_>, row: u16) {
        match self {
            Self::Gap => {}
            Self::Said {
                time,
                name,
                name_role,
                text_role,
                lines,
            } => {
                // The time is written from the gutter and the name from the
                // fixed indent, so a short or missing time never shifts the
                // name — the furniture column stays a clean gutter.
                grid.put(GUTTER, row, &format!(" {time}"), Role::Furniture.style());
                grid.put(INDENT, row, name, name_role.style());
                grid.lines(INDENT, row + 1, lines, text_role.style());
            }
            Self::Recalled { card, quote } => {
                let (col, width) = RECALL;
                let mut inner = Panel::new(&card.source, Kind::Returned)
                    .badge(&card.because)
                    .draw(grid, Place::new(col, row, width, self.height()));
                inner.lines(1, 0, quote, Role::Strongest.style());
            }
            Self::Cautioned { card, lead } => {
                let (col, width) = CAUTION;
                let mut inner = Panel::new(&card.title, Kind::Caution)
                    .badge(&card.because)
                    .draw(grid, Place::new(col, row, width, self.height()));
                let mut at = 0;
                for line in lead {
                    inner.run(1, at, emphasise_quoted(line));
                    at = at.saturating_add(1);
                }
                // A blank row separates the statement from what leans on it.
                inner.lines(
                    LEANING_INDENT,
                    at.saturating_add(1),
                    &card.leaning,
                    Role::Fading.style(),
                );
            }
        }
    }
}

/// A row count as a `u16`, saturating rather than wrapping on a pathological
/// input — a turn with 70 000 lines still measures sanely.
fn rows(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

/// Split a line into ordinary text and quoted text, brightening the quotation.
///
/// The design emphasises the commitment inside a caution — `You've held to
/// "block, never drop" every day this week` — with the quoted words in the
/// brightest step. Finding it here rather than marking it up in the model keeps
/// the model prose: the emphasis is a property of how a caution is drawn, not of
/// what it says.
///
/// An unclosed quotation mark leaves the rest of the line emphasised, which is
/// the benign reading. Nothing is dropped either way — every character of the
/// input appears in the output.
fn emphasise_quoted(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, piece) in line.split('"').enumerate() {
        if index > 0 {
            // The mark belongs with the quotation it delimits.
            spans.push(Span::styled("\"", Role::Strongest.style()));
        }
        if !piece.is_empty() {
            let inside = index % 2 == 1;
            let role = if inside { Role::Strongest } else { Role::Body };
            spans.push(Span::styled(piece.to_owned(), role.style()));
        }
    }
    spans
}

/// Turn a conversation into blocks, wrapped to `width` columns of text.
fn blocks(conversation: &Conversation, person: &str, width: u16) -> Vec<Block> {
    let mut blocks = Vec::new();
    for (index, turn) in conversation.turns.iter().enumerate() {
        if index > 0 {
            blocks.push(Block::Gap);
        }
        blocks.push(match turn {
            Turn::Said {
                time,
                speaker,
                text,
            } => {
                let (name, name_role, text_role) = match speaker {
                    // The person's own words are the brightest thing in the
                    // panel; Mooshik's are body text under an accent name, so a
                    // glance separates what I said from what it said back.
                    Speaker::Person => (person.to_owned(), Role::Strongest, Role::Strongest),
                    Speaker::Mooshik => {
                        (text::get("tui.brand").to_owned(), Role::Accent, Role::Body)
                    }
                };
                Block::Said {
                    time: time.clone(),
                    name,
                    name_role,
                    text_role,
                    lines: wrap(text, width),
                }
            }
            Turn::Recalled(card) => Block::Recalled {
                card: card.clone(),
                quote: wrap(&card.quote, RECALL.1.saturating_sub(CARD_CHROME)),
            },
            Turn::Cautioned(card) => Block::Cautioned {
                card: card.clone(),
                lead: wrap(&card.lead, CAUTION.1.saturating_sub(CARD_CHROME)),
            },
        });
    }
    blocks
}

/// Keep the last whole blocks that fit in `height` rows.
fn fit(blocks: Vec<Block>, height: u16) -> Vec<Block> {
    let mut kept = Vec::new();
    let mut used = 0u16;
    for block in blocks.into_iter().rev() {
        let next = used.saturating_add(block.height());
        if next > height {
            break;
        }
        used = next;
        kept.push(block);
    }
    kept.reverse();
    // A leading gap reads as a stray blank row at the top of the panel.
    if matches!(kept.first(), Some(Block::Gap)) {
        kept.remove(0);
    }
    kept
}

/// Draw the conversation panel over `(col, row)`..`+(w, h)` of `grid`.
///
/// `conversation.earlier` is the elision marker, set by whoever fills the model
/// when there is history above what fits. It reserves its own two rows whether
/// or not this particular draw also has to trim, because the marker is a
/// statement about the day rather than about the panel's height.
pub fn panel(
    grid: &mut Grid<'_>,
    person: &str,
    conversation: &Conversation,
    focused: bool,
    at: Place,
) {
    let mut inner = Panel::new(
        text::get("tui.panel_conversation"),
        Kind::focused_if(focused),
    )
    .draw(grid, at);

    let top = if let Some(marker) = &conversation.earlier {
        inner.put(INDENT, 0, marker, Role::Furniture.style());
        ELISION_ROWS
    } else {
        0
    };

    let text_width = inner.width().saturating_sub(INDENT);
    let available = inner.height().saturating_sub(top);
    let blocks = fit(blocks(conversation, person, text_width), available);

    // The newest turn sits on the panel's bottom rule, just above the composer,
    // and any slack opens up under the elision marker instead. Dropping whole
    // turns means the kept ones rarely fill the panel exactly, and top-anchoring
    // left those spare rows at the bottom — a gap between the last thing said
    // and the box you say the next thing in, which reads as a rendering fault.
    let used: u16 = blocks
        .iter()
        .map(Block::height)
        .fold(0u16, |sum, height| sum.saturating_add(height));
    let mut at = top.saturating_add(available.saturating_sub(used));

    for block in &blocks {
        block.draw(&mut inner, at);
        at = at.saturating_add(block.height());
    }
}

/// Draw the composer: the prompt, the draft, the cursor, and the line that says
/// nothing here needs saving.
///
/// The reassurance is part of the panel because the design puts it there:
/// nothing in the conversation is a form, so there is no save to look for.
pub fn composer(grid: &mut Grid<'_>, conversation: &Conversation, focused: bool, at: Place) {
    let mut inner =
        Panel::new(text::get("tui.panel_composer"), Kind::focused_if(focused)).draw(grid, at);

    let draft = &conversation.composer.draft;
    let mut spans = vec![Span::styled("▌", Role::Accent.style())];
    if !draft.is_empty() {
        spans.push(Span::styled(format!(" {draft}"), Role::Strongest.style()));
    }
    spans.push(Span::styled("█", Role::Accent.style()));
    inner.run(1, 0, spans);

    // The hint takes the panel's last interior row, and only when there is one
    // to spare — the narrow layout's three-row composer has none.
    let last = inner.height().saturating_sub(1);
    if last > 0 {
        super::chrome::note_rule(&mut inner, 3, last, text::get("tui.hint_composer"));
    }
}

#[cfg(test)]
mod tests {
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
        let conversation =
            conversation_of(vec![said("09:04", Speaker::Person, "Postmortem's done.")]);
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
    #[test]
    fn a_panel_with_no_interior_draws_nothing() {
        let conversation = conversation_of(vec![said("09:04", Speaker::Person, "x")]);
        let buf = drawn(4, 2, |grid| {
            panel(grid, "Neom", &conversation, false, Place::new(0, 0, 2, 2));
            composer(grid, &conversation, false, Place::new(2, 0, 2, 2));
        });
        assert_eq!(buf.area.width, 4);
    }
}
