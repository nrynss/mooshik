//! The artboards, one module each, and the geometry they share.
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

use chrome::Margins;

/// Cells kept clear between wrapped text and the panel rule on its right.
///
/// Every artboard leaves space there, and it is not slack: `1a`'s thread panel
/// has 35 columns available and its longest line is 29, and its conversation has
/// 61 available for a longest line of 52. Wrapping to the full available width
/// instead put a word or a comma against the rule, which reads as text running
/// into the frame. Two cells is the smallest gap that still reads as a margin,
/// and it is small enough that no artboard line reflows because of it.
pub const RIGHT_MARGIN: u16 = 2;

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
    /// The panels `Tab` cycles through, in order.
    pub const CYCLE: [Self; 4] = [
        Self::Conversation,
        Self::Today,
        Self::Threads,
        Self::Trickle,
    ];

    /// The next panel in the cycle, wrapping.
    pub fn next(self) -> Self {
        let at = Self::CYCLE.iter().position(|f| *f == self).unwrap_or(0);
        Self::CYCLE[(at + 1) % Self::CYCLE.len()]
    }

    /// The previous panel in the cycle, wrapping.
    pub fn previous(self) -> Self {
        let at = Self::CYCLE.iter().position(|f| *f == self).unwrap_or(0);
        Self::CYCLE[(at + Self::CYCLE.len() - 1) % Self::CYCLE.len()]
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
        }
    }

    /// How many rows panels may use.
    pub fn rows(self) -> u16 {
        self.bottom.saturating_sub(self.top)
    }
}
