//! Word wrapping with a hanging indent, because the same sentence renders at
//! three different widths.
//!
//! A day's entries appear in a 15-column week gutter, the 46-column Today panel
//! and the 44-column week detail pane; a thread's summary appears at roughly 30
//! columns on the Today panel and 40 on the week screen. Carrying pre-wrapped
//! lines in the model would mean one `Day` per width, and the first panel resize
//! would leave the other two wrong. So the model carries prose and the screens
//! wrap it here.
//!
//! The hanging indent is what makes a timed entry readable:
//!
//! ```text
//!  09:42  The ring overflowed in
//!         production
//! ```
//!
//! The continuation lines up under the text, not under the timestamp, so the
//! furniture column stays a clean gutter. [`wrap`] returns only the text lines
//! and leaves the indent to the caller's column arithmetic, which is how the
//! artboards place it.

/// Wrap `text` to `width` **characters**, breaking on spaces.
///
/// Characters, not columns: every line this returns is at most `width` `char`s
/// long, which is the same thing only while the text is single-width. A
/// double-width glyph — CJK, or an emoji — advances two cells and would push
/// its line one column past `width`, and a combining mark would leave it a
/// column short. Measuring properly needs a width table, which is a dependency
/// this crate does not take; the artboards' prose is Latin text and the model's
/// content is the user's own writing, so the trade is stated rather than made
/// silently. The grid clips either way, so the failure is a clipped glyph and
/// never a wrapped row.
///
/// A word longer than `width` is hard-broken rather than allowed to overflow —
/// clipping is the design's rule ("Nothing scrolls sideways") and a 60-character
/// path in a 30-column panel must still show its first 30 characters rather than
/// vanish. `width` of zero yields nothing, so a panel with no interior draws no
/// text instead of looping forever.
pub fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width);
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = word.chars().count();
        // A word that cannot fit on any line is broken across as many as it
        // needs, so the text is clipped by the panel rather than by this.
        if word_width > width {
            if line_width > 0 {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            for chunk in chunks(word, width) {
                lines.push(chunk);
            }
            continue;
        }
        let needed = if line_width == 0 {
            word_width
        } else {
            line_width + 1 + word_width
        };
        if needed > width {
            lines.push(std::mem::take(&mut line));
            line_width = 0;
        }
        if line_width > 0 {
            line.push(' ');
            line_width += 1;
        }
        line.push_str(word);
        line_width += word_width;
    }
    if line_width > 0 {
        lines.push(line);
    }
    lines
}

/// Wrap each paragraph of `text` separately, keeping blank lines between them.
///
/// The week screen's trailing notes are two paragraphs with a gap — "You came
/// back to the 512 cap four times on this day." then "You still haven't called
/// him back — it's come up on two days since." — and the gap is the whole
/// reason they read as two separate observations rather than one.
pub fn wrap_paragraphs(text: &str, width: u16) -> Vec<String> {
    // Checked before the loop, or a zero-width call would still emit the blank
    // separator rows and report lines a panel with no interior cannot draw.
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for (index, paragraph) in text.split("\n\n").enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        lines.extend(wrap(paragraph, width));
    }
    lines
}

/// `text` on **one** line, marked with [`marks::ELLIPSIS`] where it was cut.
///
/// For the places the design gives a list one row per item — the collapsed
/// thread line on `1h`, and the dependency lists inside `1d`'s caution card and
/// its "What leans on this" panel. Those were written straight through
/// [`Grid::lines`](crate::tui::grid::Grid::lines), which clips at the panel edge
/// and leaves no mark, so a 45-column terminal drew `1d`'s "The 40ms fairness
/// quantum assumes writers wait" as `The 40ms fairness quantum assum` — a name
/// the reader has no reason to doubt and which is not the name.
///
/// The wrap is one cell short of `room` so the mark itself has somewhere to go.
/// An entry that fits comes back untouched, mark and all, so a list of short
/// names is not peppered with ellipses that mean nothing.
pub fn ellipsised(text: &str, room: u16) -> String {
    let mut lines = wrap(text, room.saturating_sub(1)).into_iter();
    match (lines.next(), lines.next()) {
        (Some(line), Some(_)) => format!("{line}{}", crate::tui::widget::marks::ELLIPSIS),
        (Some(line), None) => line,
        (None, _) => String::new(),
    }
}

/// Split `word` into `width`-character pieces.
fn chunks(word: &str, width: usize) -> Vec<String> {
    let characters: Vec<char> = word.chars().collect();
    characters
        .chunks(width)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The case the artboards are full of: prose broken to a panel's width.
    #[test]
    fn prose_breaks_on_spaces() {
        assert_eq!(
            wrap(
                "The ring holds 512 in flight; overflow blocks, never drops",
                30
            ),
            [
                "The ring holds 512 in flight;",
                "overflow blocks, never drops"
            ]
        );
    }

    /// A line that exactly fills the width is not broken early — an off-by-one
    /// here would reflow every panel in the app by a word.
    #[test]
    fn a_line_may_fill_the_width_exactly() {
        assert_eq!(wrap("abcd efgh", 9), ["abcd efgh"]);
        assert_eq!(wrap("abcd efgh", 8), ["abcd", "efgh"]);
    }

    /// A word too long for the panel is hard-broken so its beginning still
    /// shows. The alternative — letting it overflow — would scroll sideways,
    /// which the design forbids.
    #[test]
    fn an_overlong_word_is_broken_rather_than_overflowing() {
        assert_eq!(
            wrap("/srv/quillstone/build/cache", 10),
            ["/srv/quill", "stone/buil", "d/cache"]
        );
        for line in wrap("/srv/quillstone/build/cache", 10) {
            assert!(line.chars().count() <= 10);
        }
    }

    /// An overlong word after existing text starts on its own line rather than
    /// being appended to a full one.
    #[test]
    fn an_overlong_word_starts_a_fresh_line() {
        assert_eq!(wrap("at /srv/quillstone", 8), ["at", "/srv/qui", "llstone"]);
    }

    /// Zero width yields nothing rather than looping — a panel with no interior
    /// simply draws no text.
    #[test]
    fn zero_width_yields_nothing() {
        assert!(wrap("anything at all", 0).is_empty());
        assert!(wrap_paragraphs("anything\n\nat all", 0).is_empty());
    }

    /// Runs of whitespace collapse and empty input yields no lines, so a missing
    /// field draws nothing instead of a blank row.
    #[test]
    fn empty_and_whitespace_input_yield_no_lines() {
        assert!(wrap("", 20).is_empty());
        assert!(wrap("   \t  ", 20).is_empty());
        assert_eq!(wrap("a    b", 20), ["a b"]);
    }

    /// The gap between paragraphs survives wrapping — it is what makes the
    /// week screen's notes read as two observations rather than one.
    #[test]
    fn paragraphs_keep_the_blank_line_between_them() {
        let text = "You came back to the 512 cap four times on this day.\n\nYou still haven't called him back.";
        let lines = wrap_paragraphs(text, 34);
        assert_eq!(
            lines,
            [
                "You came back to the 512 cap four",
                "times on this day.",
                "",
                "You still haven't called him back.",
            ]
        );
    }

    /// A one-line fit is left alone; anything longer says it was cut, and never
    /// spills past the room it was given.
    ///
    /// The mark matters: `Grid::lines` clipped these at the panel edge and said
    /// nothing, so `1d`'s "The 40ms fairness quantum assumes writers wait" read
    /// as a name ending in "assum".
    #[test]
    fn an_ellipsis_marks_a_line_that_had_to_be_cut() {
        assert_eq!(ellipsised("Short name", 20), "Short name");
        // Exactly the room, less the cell the mark would need: still whole.
        assert_eq!(ellipsised("Short name", 11), "Short name");
        let cut = ellipsised("The 40ms fairness quantum assumes writers wait", 20);
        assert!(cut.ends_with('…'), "{cut:?}");
        assert!(cut.chars().count() <= 20, "{cut:?}");
        assert!(!cut.contains("writers"), "{cut:?}");
        // No room at all is nothing, not a bare mark.
        assert_eq!(ellipsised("anything", 0), "");
        assert_eq!(ellipsised("", 20), "");
    }

    /// Nothing wrapped ever exceeds the width it was given, at any width.
    #[test]
    fn no_wrapped_line_exceeds_the_width() {
        let text = "Secrets never get remembered — the vault is the only place a credential lives";
        for width in 1..=60u16 {
            for line in wrap(text, width) {
                assert!(
                    line.chars().count() <= usize::from(width),
                    "{line:?} exceeds {width}"
                );
            }
        }
    }
}
