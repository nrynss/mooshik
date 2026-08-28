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
//!
//! The one exception is a turn taller than the whole panel: dropping *that*
//! left the panel empty behind its frame, with nothing to scroll it back. See
//! [`Block::clip_to`], which also states the floor — a block that still does not
//! fit once clipped is dropped after all, because a half-drawn frame is worse
//! than an empty panel.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Caution, Conversation, Recall, Speaker, Turn},
        theme::Role,
        widget::{Kind, Panel},
        wrap::{ellipsised, wrap},
    },
};

use super::RIGHT_MARGIN;

/// The composer's prompt and its cursor.
///
/// Glyphs, so they live in Rust rather than in `en.toml` — they are `1i`'s
/// notation and mean the same in every locale. The rule is stated in full in
/// [`crate::tui::widget::marks`], where the rest of the glyphs are.
const PROMPT: &str = "▌";
const CURSOR: &str = "█";

/// The column continuation lines and speaker names sit at, inside the panel's
/// interior. The design's `--cw * 10` against a panel at column 0.
const INDENT: u16 = 9;
/// The column the timestamp gutter starts at.
const GUTTER: u16 = 1;

/// Where a recall card sits and how wide it is, inside the panel's interior.
/// The design's `col=10 w=54` against a panel at column 0.
///
/// The width is a *maximum*, not a fixed size — see [`card_width`].
const RECALL: (u16, u16) = (INDENT, 54);
/// The same for a caution, which sits two columns further left and four wider —
/// it is the more serious of the two and reads that way.
const CAUTION: (u16, u16) = (INDENT - 2, 58);
/// Indent of the "what leans on this" list inside a caution card.
const LEANING_INDENT: u16 = 3;
/// Columns a card's frame and inset take off its text width: two rules and the
/// one-cell inset its body is written at.
///
/// This is the *chrome*, not the margin. Subtracting it alone left the body
/// wrapping to the exact cell the right rule sits beside, so `1d`'s three breaks
/// were all lost and its second line came out 55 characters in a 55-cell run,
/// touching the frame — the fault `week`'s own margins were written to close
/// ("`mid-incident —` against the rule"), on the one artboard whose entire
/// content is this card. Both bodies take [`RIGHT_MARGIN`] as well, which is what
/// the panels around them do.
const CARD_CHROME: u16 = 3;

/// The width a card's body is wrapped to, given the card's drawn width.
///
/// Two cells of margin is the only value that reproduces both artboards from the
/// low end, the way every other margin here is chosen. `1d`'s card is 58 wide
/// with its text at column 10 and its rule at 65 — 55 cells — and its lines are
/// 49, 48 and 29 characters with the next word taking two of them to 54, so the
/// artboard's own width is somewhere in 49..=53; 53 lands on all three breaks.
/// `1c`'s card is 54 wide, its two lines are 48 each and their next words reach
/// 52 and 53, so 49 keeps both. Anything wider loses a break; anything narrower
/// starts inventing them.
fn body_width(drawn: u16) -> u16 {
    drawn
        .saturating_sub(CARD_CHROME)
        .saturating_sub(RIGHT_MARGIN)
}
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
    ///
    /// Saturating throughout, because [`rows`] saturates: a plain `1 + rows(..)`
    /// panicked in debug on a turn of `u16::MAX` lines and, worse, wrapped to 0
    /// in release — a block of height 0 "fits" any panel, [`fit`] then keeps
    /// every block there is, and the draw loop stops advancing.
    fn height(&self) -> u16 {
        match self {
            Self::Gap => 1,
            // The speaker's row, then the words.
            Self::Said { lines, .. } => rows(lines.len()).saturating_add(1),
            // Two rules, and the quote between them.
            Self::Recalled { quote, .. } => rows(quote.len()).saturating_add(2),
            // Two rules, the lead, a blank, the list, and a blank before the
            // badge — the shape artboard `1d` draws.
            Self::Cautioned { card, lead } => rows(lead.len())
                .saturating_add(rows(card.leaning.len()))
                .saturating_add(CAUTION_CHROME),
        }
    }

    /// Trim this block so it fits in `height` rows.
    ///
    /// Only reached when the newest block *alone* is taller than the panel — a
    /// long reply into a short terminal; about 1900 characters into a 31-row
    /// interior does it. [`fit`] used to drop it like any other block that did
    /// not fit, which left `kept` empty and the panel showing nothing but its
    /// own frame, with no key that scrolls it back. Keeping the block and
    /// letting the panel clip it is what the design asks for: the panel "shows
    /// the tail that fits".
    ///
    /// **A turn is trimmed from the front; a card is trimmed from the back.** A
    /// turn is a stream and reads from its last word — the newest words are the
    /// ones the reader is waiting for, and they are the ones that would otherwise
    /// fall off the bottom. A card is a *statement* and reads from its first: a
    /// caution clipped from the front began mid-sentence ("every day this week —
    /// it's the thing you come back to most"), which is the same fault as a
    /// quotation without its attribution, and this module refuses that outright.
    ///
    /// What is never dropped either way is the chrome that makes the rest
    /// legible: the speaker's row, and a card's two rules with its badge. Which
    /// is also the floor, and why this returns an `Option`: below that chrome
    /// there is nothing left to clip to, and it **declines** rather than handing
    /// back a block with no content in it.
    ///
    /// It used to return the block regardless, so a recall clipped to one row came
    /// back still measuring two, [`fit`] pushed it, and the grid clipped away the
    /// card's bottom rule and its badge — precisely the half-frame this exists to
    /// prevent, and reachable at 120x9 on the wide layout or 80x11 on the narrow
    /// one. What was drawn there was a frame with no title, no quote and no
    /// reason: strictly less than nothing.
    fn clip_to(self, height: u16) -> Option<Self> {
        match self {
            Self::Gap => (height >= 1).then_some(Self::Gap),
            Self::Said {
                time,
                name,
                name_role,
                text_role,
                lines,
            } => {
                // The speaker's row, and at least one row of what they said: a
                // name with its words trimmed away says who spoke and not what.
                let room = height.checked_sub(1).filter(|room| *room > 0)?;
                Some(Self::Said {
                    time,
                    name,
                    name_role,
                    text_role,
                    lines: keep_tail(lines, room),
                })
            }
            Self::Recalled { card, mut quote } => {
                let room = height.checked_sub(2).filter(|room| *room > 0)?;
                quote.truncate(usize::from(room));
                Some(Self::Recalled { card, quote })
            }
            Self::Cautioned { mut card, mut lead } => {
                // The lead is the statement, so it is trimmed last: the list of
                // what leans on the commitment goes first.
                let room = height
                    .checked_sub(CAUTION_CHROME)
                    .filter(|room| *room > 0)?;
                lead.truncate(usize::from(room));
                let mut left = room.saturating_sub(rows(lead.len()));
                // But never all the way to nothing while the lead is still
                // pointing at it. The lead's last line *is* the colon that
                // introduces the list — `1d` ends it "eight other notes lean on
                // it:" — so trimming the list to zero left a card whose sentence
                // announced a list and then showed none, which is the same fault
                // as a bare "· Just remembered:". One name makes the colon true,
                // so where the list would get no rows at all the lead gives up
                // its last one for it — and its last one is the colon.
                //
                // Only where the list would get *nothing*: a list that already
                // fits keeps the whole lead, which is what makes this a floor
                // under the trimming order rather than a change to it.
                if left == 0 && !card.leaning.is_empty() {
                    lead.truncate(lead.len().saturating_sub(1));
                    left = room.saturating_sub(rows(lead.len()));
                }
                card.leaning.truncate(usize::from(left));
                Some(Self::Cautioned { card, lead })
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
                grid.lines(INDENT, row.saturating_add(1), lines, text_role.style());
            }
            Self::Recalled { card, quote } => {
                let (col, design) = RECALL;
                let width = card_width(design, grid.width().saturating_sub(col));
                let mut inner = Panel::new(&card.source, Kind::Returned)
                    .badge(&card.because)
                    .draw(grid, Place::new(col, row, width, self.height()));
                inner.lines(1, 0, quote, Role::Strongest.style());
            }
            Self::Cautioned { card, lead } => {
                let (col, design) = CAUTION;
                let width = card_width(design, grid.width().saturating_sub(col));
                // The title is fixed chrome, not content: `1d`'s " One thing
                // before you do " is the same sentence on every caution there
                // could be, so it comes from `en.toml` beside every other panel
                // title rather than from the model.
                let mut inner = Panel::new(text::get("tui.panel_caution"), Kind::Caution)
                    .badge(&card.because)
                    .draw(grid, Place::new(col, row, width, self.height()));
                // One state for the whole block, threaded through the lines: see
                // `emphasise_quoted`.
                let mut inside = false;
                let mut at = 0;
                for line in lead {
                    inner.run(1, at, emphasise_quoted(line, &mut inside));
                    at = at.saturating_add(1);
                }
                // A blank row separates the statement from what leans on it.
                //
                // Ellipsised rather than clipped: these are names of other notes,
                // as long as whatever the user called them, and `Grid::lines`
                // cut them at the card's edge with no mark at all. `1d`'s own
                // "The 40ms fairness quantum assumes writers wait" came out as
                // "The 40ms fairness quantum assum" between 45 and 60 columns —
                // a name the reader has no reason to doubt, and not the name.
                let room = inner
                    .width()
                    .saturating_sub(LEANING_INDENT)
                    .saturating_sub(RIGHT_MARGIN);
                let leaning: Vec<String> = card
                    .leaning
                    .iter()
                    .map(|name| ellipsised(name, room))
                    .collect();
                inner.lines(
                    LEANING_INDENT,
                    at.saturating_add(1),
                    &leaning,
                    Role::Fading.style(),
                );
            }
        }
    }
}

/// Rows a caution card spends on chrome: two rules, the blank before the list
/// of what leans on it, and the blank before the badge — artboard `1d`'s shape.
const CAUTION_CHROME: u16 = 4;

/// A row count as a `u16`, saturating rather than wrapping on a pathological
/// input — a turn with 70 000 lines still measures sanely.
fn rows(count: usize) -> u16 {
    u16::try_from(count).unwrap_or(u16::MAX)
}

/// The last `room` of `lines`, dropping from the front — how a *turn* is
/// clipped. A card is clipped the other way round; see [`Block::clip_to`].
fn keep_tail(mut lines: Vec<String>, room: u16) -> Vec<String> {
    let room = usize::from(room);
    if lines.len() > room {
        lines.drain(..lines.len() - room);
    }
    lines
}

/// How wide a card may be inside a panel `available` columns across.
///
/// The artboards' `w=54` and `w=58` are the widths at 120 columns and were being
/// used unconditionally, so on a 60-column terminal the card was drawn wider
/// than the panel: the quote clipped mid-word with no ellipsis and the frame lost
/// its right edge, because the grid clipped the rule away. Taking the smaller of
/// the two keeps the design's width where there is room for it and a whole frame
/// where there is not.
fn card_width(design: u16, available: u16) -> u16 {
    design.min(available)
}

/// Split a line into ordinary text and quoted text, brightening the quotation.
///
/// The design emphasises the commitment inside a caution — `You've held to
/// "block, never drop" every day this week` — with the quoted words in the
/// brightest step. Finding it here rather than marking it up in the model keeps
/// the model prose: the emphasis is a property of how a caution is drawn, not of
/// what it says.
///
/// **`inside` is the caller's, and it spans the whole block.** A quotation is a
/// property of the caution, not of a line, and the caution's lead wraps: at 40
/// columns `1d`'s opening breaks as `You've held to "block, never` / `drop" every
/// day this week —`. This used to derive the state from `index % 2` within one
/// line, which restarts at "outside" on every line, so the second line came out
/// exactly inverted — `drop` in body text and the words after the closing mark in
/// the brightest step. The caller resets it once per block; see
/// [`Block::draw`].
///
/// An unclosed quotation mark leaves the rest of the block emphasised, which is
/// the benign reading. Nothing is dropped either way — every character of the
/// input appears in the output.
fn emphasise_quoted(line: &str, inside: &mut bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (index, piece) in line.split('"').enumerate() {
        if index > 0 {
            // The mark belongs with the quotation it delimits, whichever end of
            // it this one is.
            spans.push(Span::styled("\"", Role::Strongest.style()));
            *inside = !*inside;
        }
        if !piece.is_empty() {
            let role = if *inside { Role::Strongest } else { Role::Body };
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
            // The cards are wrapped to the width they will actually be drawn
            // at, which is the design's width only while the panel has room
            // for it — `width` here is the panel's own text width, so
            // `width + INDENT` recovers the interior it came from.
            Turn::Recalled(card) => {
                let interior = width.saturating_add(INDENT).saturating_add(RIGHT_MARGIN);
                let drawn = card_width(RECALL.1, interior.saturating_sub(RECALL.0));
                Block::Recalled {
                    card: card.clone(),
                    quote: wrap(&card.quote, body_width(drawn)),
                }
            }
            Turn::Cautioned(card) => {
                let interior = width.saturating_add(INDENT).saturating_add(RIGHT_MARGIN);
                let drawn = card_width(CAUTION.1, interior.saturating_sub(CAUTION.0));
                Block::Cautioned {
                    card: card.clone(),
                    lead: wrap(&card.lead, body_width(drawn)),
                }
            }
        });
    }
    blocks
}

/// Keep the last whole blocks that fit in `height` rows.
///
/// The newest block is kept even when it does not fit, clipped to the panel —
/// otherwise a single long reply produced an empty panel behind its frame, and
/// nothing in the app scrolls it back. See [`Block::clip_to`].
///
/// **And only when clipping leaves something worth drawing.** [`Block::clip_to`]
/// cannot shrink a block below its own chrome — a recall needs two rules, a
/// caution four rows before a word of it is legible — and it declines rather than
/// returning a block with nothing in it. The result is also re-measured here, so
/// the two independent pieces of arithmetic (`clip_to`'s room and
/// [`Block::height`]) have to agree before anything is drawn.
///
/// So a panel with a one-row interior and nothing but a card in it draws its own
/// frame and nothing else: "never leave the panel empty" yields to "never draw a
/// broken frame", because an empty panel is at least a whole one.
fn fit(blocks: Vec<Block>, height: u16) -> Vec<Block> {
    let mut kept = Vec::new();
    let mut used = 0u16;
    let mut oversized = None;
    for block in blocks.into_iter().rev() {
        let next = used.saturating_add(block.height());
        if next > height {
            // The first block seen in reverse is the newest one. If *it* is
            // what did not fit, there is nothing else to fall back on.
            if kept.is_empty() {
                oversized = Some(block);
            }
            break;
        }
        used = next;
        kept.push(block);
    }
    if let Some(block) = oversized
        .and_then(|block| block.clip_to(height))
        .filter(|block| block.height() <= height)
    {
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

    let text_width = inner
        .width()
        .saturating_sub(INDENT)
        .saturating_sub(RIGHT_MARGIN);
    let available = inner.height().saturating_sub(top);
    let blocks = fit(blocks(conversation, person, text_width), available);

    // Top-anchored, and the slack is left at the bottom, because that is what the
    // artboard draws: `1a` puts the elision marker on row 2, one blank on row 3,
    // the first turn on row 4, and its last line on row 30 of an interior that
    // runs to row 32 — two spare rows above the composer. Bottom-anchoring closed
    // that gap and looked tidier, but the gap is the designer's: the conversation
    // grows downwards into it, and pinning the newest turn to the rule meant every
    // reply shifted the whole scroll up by its own height. What the reader needs
    // is that the newest turn survives the trim, and [`fit`] is what provides
    // that — it keeps the tail and drops from the front.
    let mut at = top;

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
    let mut spans = vec![Span::styled(PROMPT, Role::Accent.style())];
    if !draft.is_empty() {
        spans.push(Span::styled(format!(" {draft}"), Role::Strongest.style()));
    }
    spans.push(Span::styled(CURSOR, Role::Accent.style()));
    inner.run(1, 0, spans);

    // The hint takes the panel's last interior row, and only when there is one
    // to spare — the narrow layout's three-row composer has none.
    let last = inner.height().saturating_sub(1);
    if last > 0 {
        super::chrome::note_rule(&mut inner, 3, last, text::get("tui.hint_composer"));
    }
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
