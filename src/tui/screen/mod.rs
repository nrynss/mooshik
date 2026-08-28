//! The ported artboards, one module each, and the geometry they share.
//!
//! | Module               | Artboards  | What it draws                        |
//! | -------------------- | ---------- | ------------------------------------ |
//! | [`chrome`]           | all        | the title rule and the bottom rule   |
//! | [`conversation`]     | 1a, 1c, 1d | the conversation panel and composer |
//! | [`aside`]            | 1a, 1d     | the three right-hand panels          |
//! | [`today`]            | 1a, 1c, 1d | the default screen                   |
//! | [`week`]             | 1b         | seven days and the threads across    |
//! | [`narrow`]           | 1h         | the same day at 80x24                |
//!
//! `1e`, `1f` and `1g` have no module because they are not ported — they are
//! settings and lifecycle screens rather than the companion surface, and
//! [`crate::tui`]'s header says why.
//!
//! `1c` and `1d` have no module of their own on purpose. They are not modes: the
//! recall card and the caution are [`Turn`](super::model::Turn) variants inside
//! the ordinary conversation, so [`today`] draws all three artboards from the
//! same code and the difference lives in the data. That is the design's argument
//! made structural — a caution that needed its own screen would be the modal the
//! design refuses to draw.

pub mod aside;
pub mod chrome;
pub mod conversation;
pub mod narrow;
pub mod today;
pub mod week;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod helper_tests {
    use super::{joined, spelled, Focus};

    /// A separator is drawn only between two things that are both there.
    #[test]
    fn joining_skips_the_parts_that_are_not_there() {
        assert_eq!(
            joined(&["Your week", "21-27 August"], " · "),
            "Your week · 21-27 August"
        );
        assert_eq!(joined(&["Your week", ""], " · "), "Your week");
        assert_eq!(joined(&["", "21-27 August"], " · "), "21-27 August");
        assert_eq!(joined(&["", ""], " · "), "");
        // Whitespace is nothing: the live workspace's fields are `String::new()`
        // today, but a source that fills one with a space must not draw a bullet.
        assert_eq!(joined(&["a", "   ", "b"], " · "), "a · b");
        assert_eq!(joined(&["a", "b", "c"], " · "), "a · b · c");
    }

    /// Small numbers are spelled, as every artboard spells them, and large ones
    /// fall back to the numeral rather than panicking on a missing key.
    #[test]
    fn small_numbers_are_spelled_and_large_ones_are_not() {
        assert_eq!(spelled(1), "one");
        assert_eq!(spelled(4), "four");
        assert_eq!(spelled(8), "eight");
        assert_eq!(spelled(12), "twelve");
        assert_eq!(spelled(13), "13");
        assert_eq!(spelled(214), "214");
        // Zero has no word in the table, and no caller reaches it with a count
        // worth spelling — it must still not panic.
        assert_eq!(spelled(0), "0");
    }

    /// A sentence that starts with the number gets the capital, as `1d` writes
    /// it, and the numeral fallback is untouched by the casing.
    #[test]
    fn a_leading_number_is_capitalised() {
        use super::spelled_leading;
        assert_eq!(spelled_leading(8), "Eight");
        assert_eq!(spelled_leading(1), "One");
        assert_eq!(spelled_leading(12), "Twelve");
        assert_eq!(spelled_leading(13), "13");
        assert_eq!(spelled_leading(0), "0");
        // And the lowercase form is still lowercase, for `1h`'s "four more".
        assert_eq!(spelled(8), "eight");
    }

    /// The narrow cycle is one panel long, so `Tab` on `1h` moves nothing, and a
    /// focus the current screen cannot draw snaps to one it can.
    #[test]
    fn a_cycle_only_reaches_the_panels_its_screen_draws() {
        assert_eq!(Focus::NARROW.len(), 1);
        assert_eq!(
            Focus::Conversation.next_in(&Focus::NARROW),
            Focus::Conversation
        );
        assert_eq!(
            Focus::Threads.next_in(&Focus::NARROW),
            Focus::Conversation,
            "a stale focus did not snap back"
        );
        assert_eq!(Focus::Threads.within(&Focus::NARROW), Focus::Conversation);
        assert_eq!(Focus::Threads.within(&Focus::CYCLE), Focus::Threads);
        // An empty cycle — the week screen — leaves focus exactly where it was.
        assert_eq!(Focus::Threads.next_in(&[]), Focus::Threads);
        assert_eq!(Focus::Threads.previous_in(&[]), Focus::Threads);
        assert_eq!(Focus::Threads.within(&[]), Focus::Threads);
        // And the wide cycle still wraps in both directions.
        assert_eq!(Focus::Trickle.next_in(&Focus::CYCLE), Focus::Conversation);
        assert_eq!(
            Focus::Conversation.previous_in(&Focus::CYCLE),
            Focus::Trickle
        );
    }
}

use chrome::Margins;

/// Cells kept clear between wrapped text and the panel rule on its right, where
/// the design leaves two.
///
/// **The margins are per panel, and the artboards say which.** They are
/// recoverable from where each artboard's lines break, and they do not agree:
/// `1a`'s conversation leaves two, `1a`'s thread panel four, `1b`'s thread panel
/// **none at all**, `1b`'s detail log four and its detail notes five. This used
/// to be one constant used everywhere, and four artboard lines reflowed as a
/// result — `1a`'s `Every day this week · eight / other notes lean on it` broke
/// after `other`, `The Quillstone cache lives on the / NAS` after `the`, `1b`'s
/// `Three days · Monday, Tuesday, / Thursday` is 38 characters with 39 available
/// and was cut to 37, and `1b`'s `09:42  The ring overflowed in / production` is
/// 33 with 34 available and came out on one line. So this is the *default*, and
/// the four panels that differ name their own margin beside the artboard line
/// that fixes it: `aside::THREAD_MARGIN`, `week::THREADS_MARGIN`,
/// `week::LOG_MARGIN` and `week::NOTES_MARGIN`.
///
/// Two is not slack where it is right: `1a`'s conversation has 61 columns
/// available for a longest line of 59 — `writers block, we don't drop. Shipping
/// the doc after lunch.` — and wrapping to the full width put that line's final
/// stop against the rule, which reads as text running into the frame.
///
/// **`1b`'s day columns contradict themselves, and this is the side that was
/// taken.** Every column there is 17 cells, so 15 of interior. Tuesday's
/// "Cobalt Lantern retries, jitter" breaks as `Cobalt Lantern / retries,
/// jitter`, and `retries, jitter` is 15 characters — the whole interior, margin
/// zero. Wednesday's "Mum called mid-incident — not called back" breaks as `Mum
/// called / mid-incident / — not called / back`, which needs a width of 13,
/// because at 15 it comes out in three lines as `mid-incident —` and `not called
/// back`. No single margin reproduces both, and the artboard is the only
/// authority there is. Two is chosen — Wednesday's — because it keeps prose off
/// the rule everywhere, which is the rule this constant exists to hold; the
/// cost is that Tuesday renders in four lines where `1b` gives it two. A margin
/// of zero would reproduce Tuesday and put Wednesday's dash against the frame on
/// the screen's hardest day.
pub const RIGHT_MARGIN: u16 = 2;

/// Join `parts` with `separator`, skipping the empty ones.
///
/// Every artboard's subject line is two or three things with a bullet between
/// them — `Mooshik  ·  Thursday 27 August  ·  14:22` — and an unconditional
/// `format!` drew the bullets whether or not the parts were there. The live
/// workspace has no date source yet, so `mooshik tui` opened on
/// `Mooshik  ·    ·  ` and the week screen's rule read ` · 214 things
/// remembered`. An absent field is a true statement about a source that does not
/// exist; a separator with nothing on either side of it is a rendering fault,
/// and it defeats the honesty argument [`crate::tui`]'s header makes.
pub fn joined(parts: &[&str], separator: &str) -> String {
    parts
        .iter()
        .filter(|part| !part.trim().is_empty())
        .copied()
        .collect::<Vec<&str>>()
        .join(separator)
}

/// `count` in words where the locale spells it, as a numeral where it does not.
///
/// The artboards spell every small number that appears in prose — `1d`'s "Eight
/// things lean on it:", `1h`'s "four more" — and a generated sentence has to
/// match, or the one line the app writes itself is the one line that reads like
/// a machine. Which numbers a language spells, and how far it goes before
/// reaching for digits, is a property of that language, so the table lives in
/// `en.toml` under `[tui.numbers]` and this is the only thing that reads it.
///
/// The fallback is the numeral, not a panic: `text::get` panics on a missing key
/// by design, so the range is checked here rather than asked for and caught.
pub fn spelled(count: usize) -> String {
    /// How far `[tui.numbers]` goes. Above this the numeral is the honest
    /// rendering — "thirteen other notes lean on it" is harder to read than 13.
    const SPELLED_TO: usize = 12;
    if (1..=SPELLED_TO).contains(&count) {
        crate::text::get(&format!("tui.numbers.{count}")).to_owned()
    } else {
        count.to_string()
    }
}

/// [`spelled`], capitalised for a sentence that starts with the number.
///
/// `1d` writes " Eight things lean on it:", so the generated header needs the
/// capital and `1h`'s "four more" needs it left alone. Which form a locale wants
/// is declared by which placeholder its string uses — `{Count}` here, `{count}`
/// for [`spelled`] — so a language that does not capitalise mid-panel simply uses
/// the other one and this is never called.
///
/// The casing is `char::to_uppercase`, which is right for the words in
/// `[tui.numbers]` and is an approximation in general: title-casing is properly a
/// locale rule, and a locale that needs a different one changes its own string
/// rather than this. The numeral fallback above twelve is unaffected by it.
pub fn spelled_leading(count: usize) -> String {
    let word = spelled(count);
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => word,
    }
}

/// Which panel holds focus, and therefore which rule is the accent.
///
/// The variants are the design's own prop list for the Today screen. Focus is
/// one accent line per screen — see [`Kind::Focused`](super::widget::Kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The conversation. Where focus starts, and where typing goes.
    #[default]
    Conversation,
    /// Today's log.
    Today,
    /// What keeps coming back.
    Threads,
    /// Just remembered.
    Trickle,
}

impl Focus {
    /// The panels `Tab` cycles through on the wide Today screen, in order.
    pub const CYCLE: [Self; 4] = [
        Self::Conversation,
        Self::Today,
        Self::Threads,
        Self::Trickle,
    ];

    /// The panels the narrow layout draws, which is the conversation and the
    /// composer under it — and those two share one focus, because the composer
    /// is where the conversation is typed rather than a panel of its own.
    ///
    /// So the narrow cycle is one element long and `Tab` moves nothing. That is
    /// the point: `1h` collapses the three right-hand panels to two summary rows
    /// and drops the third, so there is no second place for focus to be. Cycling
    /// the wide four here accented no rule anywhere, stopped keystrokes reaching
    /// the draft, and moved two cursors the screen never draws.
    pub const NARROW: [Self; 1] = [Self::Conversation];

    // There is no constant for the wide screen's other shapes. A standing caution
    // takes one stop away and a short band takes one or two more, so the set
    // depends on the terminal and on the conversation — see
    // [`today::focusable`](super::today::focusable), which derives it from the
    // same `Split` that decides a panel gets no rows. A fixed set here was how
    // the cycle and the screen came to disagree twice.

    /// The next panel in `cycle`, wrapping. A focus the cycle does not contain
    /// — left behind by a resize out of the wide layout — snaps to its first.
    pub fn next_in(self, cycle: &[Self]) -> Self {
        Self::step(self, cycle, 1)
    }

    /// The previous panel in `cycle`, wrapping.
    pub fn previous_in(self, cycle: &[Self]) -> Self {
        Self::step(self, cycle, cycle.len().saturating_sub(1))
    }

    /// This focus if `cycle` can reach it, and `cycle`'s first otherwise.
    ///
    /// A terminal narrowed from 120 to 80 columns leaves `focus` on a panel the
    /// narrow screen does not draw, which is the same fault `Tab` used to cause:
    /// no accented rule, and typing that goes nowhere. Reached from both
    /// [`crate::tui::app::App::draw`] and `is_typing`, so what is drawn and what
    /// a letter does cannot disagree.
    pub fn within(self, cycle: &[Self]) -> Self {
        if cycle.contains(&self) {
            self
        } else {
            cycle.first().copied().unwrap_or(self)
        }
    }

    fn step(self, cycle: &[Self], delta: usize) -> Self {
        if cycle.is_empty() {
            return self;
        }
        match cycle.iter().position(|f| *f == self) {
            Some(at) => cycle[(at + delta) % cycle.len()],
            // Not in this cycle at all: land on the panel the screen does draw
            // rather than stepping from a position that does not exist.
            None => cycle[0],
        }
    }
}

/// The rows a screen's panels may use, and the rows above and below them.
///
/// Every artboard has the same three bands: a title rule on row 0, panels, and a
/// bottom rule on the last row with one blank row above it. Deriving those from
/// the grid rather than hard-coding 0, 37 and 39 is what lets the same screen
/// draw correctly in a terminal that is not exactly 40 rows tall.
#[derive(Debug, Clone, Copy)]
pub struct Band {
    /// The first row panels may use. Always 1 — under the title rule.
    pub top: u16,
    /// One past the last row panels may use.
    pub bottom: u16,
    /// The bottom rule's row.
    pub status: u16,
    /// Whether there is a row for the title rule at all.
    ///
    /// False only on a one-row terminal, where it would share [`Band::status`]
    /// and the two rules would splice into each other.
    pub title: bool,
    /// Where this screen's chrome sits.
    pub margins: Margins,
}

impl Band {
    /// The band for a grid `height` rows tall, with `margins`.
    ///
    /// A grid too short for all three bands collapses to an empty panel band
    /// rather than underflowing, so a two-row terminal draws chrome and nothing
    /// else instead of panicking.
    pub fn new(height: u16, margins: Margins) -> Self {
        let status = height.saturating_sub(1);
        // The blank row above the bottom rule is the design's, not slack: it
        // keeps the rule from touching the panels it summarises.
        let bottom = height.saturating_sub(2).max(1);
        Self {
            top: 1,
            bottom,
            status,
            margins,
            // One row cannot hold two rules. It used to hold both, spliced:
            // ` ✓ oshik^2 week · Esc leave` — the health mark eating the brand
            // and the hint over the nav. Everywhere else in this app two runs
            // that cannot both fit resolve by one giving way ("one complete run
            // says more than two mangled ones"), and this was the one place
            // they did not. The bottom rule wins because it carries the keys,
            // including the one that leaves.
            title: height >= 2,
        }
    }

    /// How many rows panels may use.
    pub fn rows(self) -> u16 {
        self.bottom.saturating_sub(self.top)
    }
}
