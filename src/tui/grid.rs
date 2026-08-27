//! Absolute character placement, because that is how the design is written.
//!
//! Every artboard in `scratch_design/Mooshik TUI.dc.html` places its text as
//! `left:calc(var(--cw) * N); top:calc(var(--ch) * M)` — column `N`, row `M` of
//! a 120x40 grid — and every panel carries `col`/`row`/`w`/`h` in cells. A
//! constraint-solving layout would be a translation of that, and translations
//! drift: someone tweaks a `Constraint::Length` and a panel edge moves a column
//! away from the design with nothing to catch it.
//!
//! So [`Grid`] is the same coordinate system the design uses. `grid.put(74, 18,
//! …)` is the artboard's own `--cw * 74` / `--ch * 18`, and a screen can be
//! read side by side with the file it came from. The layout tests then assert
//! panel geometry in those same cells.
//!
//! Everything is clipped to the grid's area rather than the terminal's, so a
//! screen drawn for 120x40 in an 80-column terminal writes nothing past column
//! 79 instead of wrapping into the next row. Clipping, not wrapping, is the
//! design's own rule for the narrow case: "Nothing scrolls sideways."

use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Span, widgets::Widget};

/// Where something goes, in the design's own units: a column, a row, a width
/// and a height, all in cells.
///
/// Every panel in the artboards is specified as exactly this — `col="72" row="1"
/// w="48" h="16"` — so carrying the four together lets a call site read like the
/// file it came from, and keeps the screen functions inside a sane argument
/// count now that each of them places several panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// Column of the left edge.
    pub col: u16,
    /// Row of the top edge.
    pub row: u16,
    /// Width in cells, rules included.
    pub w: u16,
    /// Height in cells, rules included.
    pub h: u16,
}

impl Place {
    /// A placement, in the artboard's own order: `col`, `row`, `w`, `h`.
    pub const fn new(col: u16, row: u16, w: u16, h: u16) -> Self {
        Self { col, row, w, h }
    }
}

/// A character grid over some rectangle of the frame.
///
/// Cell `(0, 0)` is the top-left of `area`, not of the terminal, so a grid
/// handed a panel's interior addresses that interior directly.
pub struct Grid<'a> {
    buf: &'a mut Buffer,
    area: Rect,
}

impl<'a> Grid<'a> {
    /// A grid covering `area` of `buf`.
    pub fn new(buf: &'a mut Buffer, area: Rect) -> Self {
        Self { buf, area }
    }

    /// The grid's width in cells.
    pub fn width(&self) -> u16 {
        self.area.width
    }

    /// The grid's height in cells.
    pub fn height(&self) -> u16 {
        self.area.height
    }

    /// The rectangle this grid covers, in frame coordinates. Needed when
    /// handing a region to a ratatui widget that renders itself.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// A sub-rectangle of this grid, in frame coordinates, clipped to the grid.
    ///
    /// Clipping rather than panicking is deliberate: the same screen code runs
    /// at 120x40 and at whatever the terminal actually is, and a panel that
    /// falls off the edge should simply not draw.
    pub fn rect(&self, col: u16, row: u16, w: u16, h: u16) -> Rect {
        let x = self.area.x.saturating_add(col);
        let y = self.area.y.saturating_add(row);
        let right = self.area.right();
        let bottom = self.area.bottom();
        if x >= right || y >= bottom {
            return Rect::new(x.min(right), y.min(bottom), 0, 0);
        }
        Rect {
            x,
            y,
            width: w.min(right - x),
            height: h.min(bottom - y),
        }
    }

    /// A grid over a sub-rectangle of this one, addressed from its own origin.
    pub fn sub(&mut self, col: u16, row: u16, w: u16, h: u16) -> Grid<'_> {
        let area = self.rect(col, row, w, h);
        Grid {
            buf: self.buf,
            area,
        }
    }

    /// Write `text` at cell `(col, row)` and return the column after it.
    ///
    /// The return value is what makes a styled run readable — each fragment
    /// starts where the last one ended — and it comes from the buffer's own
    /// width accounting rather than a character count, so a wide glyph advances
    /// by the two cells it actually occupies.
    pub fn put(&mut self, col: u16, row: u16, text: &str, style: Style) -> u16 {
        let cell = self.rect(col, row, self.area.width.saturating_sub(col), 1);
        if cell.is_empty() {
            return col;
        }
        let (end_x, _) = self
            .buf
            .set_stringn(cell.x, cell.y, text, usize::from(cell.width), style);
        col.saturating_add(end_x.saturating_sub(cell.x))
    }

    /// Write styled fragments left to right from `(col, row)`, returning the
    /// column after the last one.
    ///
    /// This is the shape most of the design's text takes: one line whose parts
    /// carry different meanings, like a furniture timestamp followed by the
    /// user's own words in the brightest colour.
    pub fn run<'s>(
        &mut self,
        col: u16,
        row: u16,
        parts: impl IntoIterator<Item = Span<'s>>,
    ) -> u16 {
        let mut at = col;
        for part in parts {
            at = self.put(at, row, &part.content, part.style);
        }
        at
    }

    /// Write `text` right-aligned so it *ends* at column `end`.
    ///
    /// The design right-aligns the second half of several rules — the status
    /// bar's key hints, the week screen's scope — against the grid's own edge
    /// rather than the text's, so this takes the ending column rather than
    /// computing an offset at the call site.
    pub fn put_ending_at(&mut self, end: u16, row: u16, text: &str, style: Style) -> u16 {
        let width = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
        self.put(end.saturating_sub(width), row, text, style)
    }

    /// Write successive lines down from `(col, row)`, one per item.
    ///
    /// Returns the row after the last line written, so stacked blocks can be
    /// laid out without the call site tracking the count.
    pub fn lines<'s, I, S>(&mut self, col: u16, row: u16, lines: I, style: Style) -> u16
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str> + 's,
    {
        let mut at = row;
        for line in lines {
            if at >= self.area.height {
                break;
            }
            self.put(col, at, line.as_ref(), style);
            at = at.saturating_add(1);
        }
        at
    }

    /// Repeat `glyph` for `width` cells from `(col, row)` — a rule, or the
    /// filled side of a hand-drawn frame.
    pub fn fill(&mut self, col: u16, row: u16, width: u16, glyph: char, style: Style) {
        let run: String = std::iter::repeat_n(glyph, usize::from(width)).collect();
        self.put(col, row, &run, style);
    }

    /// Render a ratatui widget over `(col, row)`..`+(w, h)` of this grid.
    ///
    /// The buffer stays private — the grid's whole job is to make callers
    /// address cells rather than reach past it — but some things ratatui already
    /// draws correctly are not worth reimplementing cell by cell, box-drawing
    /// corner joints for both rule weights being the case that matters here.
    pub fn widget<W: Widget>(&mut self, col: u16, row: u16, w: u16, h: u16, widget: W) {
        let area = self.rect(col, row, w, h);
        if !area.is_empty() {
            widget.render(area, self.buf);
        }
    }

    /// Repeat `glyph` down `height` cells from `(col, row)`.
    pub fn fill_down(&mut self, col: u16, row: u16, height: u16, glyph: char, style: Style) {
        let mut buffer = [0u8; 4];
        let glyph = glyph.encode_utf8(&mut buffer);
        for offset in 0..height {
            self.put(col, row.saturating_add(offset), glyph, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn grid_of(width: u16, height: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, width, height))
    }

    fn row_text(buf: &Buffer, row: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, row)].symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    /// The whole point of the type: `put(col, row, …)` lands on the design's own
    /// cell, with no offset of its own.
    #[test]
    fn cells_are_the_designs_own_coordinates() {
        let mut buf = grid_of(20, 3);
        let area = buf.area;
        Grid::new(&mut buf, area).put(4, 1, "hi", Style::default());
        assert_eq!(row_text(&buf, 1).trim_end(), "    hi");
    }

    /// A sub-grid addresses from its own origin, so a panel's interior can be
    /// filled without every call site adding the panel's position back on.
    #[test]
    fn a_sub_grid_rebases_the_origin() {
        let mut buf = grid_of(20, 4);
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        grid.sub(5, 2, 10, 2).put(1, 0, "x", Style::default());
        assert_eq!(row_text(&buf, 2).trim_end(), "      x");
    }

    /// Text past the right edge is clipped, never wrapped onto the next row —
    /// the design's "Nothing scrolls sideways", and the reason a 120-column
    /// screen is safe to draw in an 80-column terminal.
    #[test]
    fn overflow_clips_instead_of_wrapping() {
        let mut buf = grid_of(8, 2);
        let area = buf.area;
        Grid::new(&mut buf, area).put(4, 0, "abcdefgh", Style::default());
        assert_eq!(row_text(&buf, 0), "    abcd");
        assert_eq!(row_text(&buf, 1), "        ");
    }

    /// Writing entirely outside the grid is a no-op rather than a panic, so a
    /// panel positioned off a small terminal simply does not draw.
    #[test]
    fn writing_outside_the_grid_does_nothing() {
        let mut buf = grid_of(6, 2);
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        assert_eq!(grid.put(20, 0, "x", Style::default()), 20);
        assert_eq!(grid.put(0, 9, "x", Style::default()), 0);
        assert_eq!(row_text(&buf, 0), "      ");
    }

    /// A run reports where it ended so the next fragment starts there, which is
    /// how a mixed-meaning line is composed.
    #[test]
    fn a_run_chains_fragments_and_keeps_their_styles() {
        let mut buf = grid_of(20, 1);
        let area = buf.area;
        let end = Grid::new(&mut buf, area).run(
            1,
            0,
            [
                Span::styled("09:04  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Neom", Style::default().fg(Color::White)),
            ],
        );
        assert_eq!(end, 12);
        assert_eq!(row_text(&buf, 0).trim_end(), " 09:04  Neom");
        assert_eq!(buf[(1, 0)].fg, Color::DarkGray);
        assert_eq!(buf[(8, 0)].fg, Color::White);
    }

    /// Right alignment takes the column the text should *end* at, matching the
    /// artboards' right-hand rules.
    #[test]
    fn text_can_end_at_a_given_column() {
        let mut buf = grid_of(12, 1);
        let area = buf.area;
        Grid::new(&mut buf, area).put_ending_at(12, 0, "keys", Style::default());
        assert_eq!(row_text(&buf, 0), "        keys");
    }

    /// Stacked lines stop at the grid's bottom instead of writing past it, and
    /// report the row they reached.
    #[test]
    fn stacked_lines_stop_at_the_bottom() {
        let mut buf = grid_of(6, 2);
        let area = buf.area;
        let next = Grid::new(&mut buf, area).lines(0, 0, ["a", "b", "c", "d"], Style::default());
        assert_eq!(next, 2);
        assert_eq!(row_text(&buf, 0), "a     ");
        assert_eq!(row_text(&buf, 1), "b     ");
    }

    /// Rules are drawn by repetition in both directions, for the frames the
    /// design draws by hand rather than through a block.
    #[test]
    fn rules_fill_across_and_down() {
        let mut buf = grid_of(6, 3);
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        grid.fill(1, 0, 4, '═', Style::default());
        grid.fill_down(0, 0, 3, '║', Style::default());
        assert_eq!(row_text(&buf, 0), "║════ ");
        assert_eq!(row_text(&buf, 2), "║     ");
    }
}
