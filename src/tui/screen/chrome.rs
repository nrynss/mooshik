//! The top and bottom rules: what Mooshik is showing, and what keys do here.
//!
//! Both rules are the same shape on every artboard — an identity on the left, a
//! right-aligned run on the right — so they live here rather than being redrawn
//! per screen. Only their content changes: the Today screen's bottom rule
//! carries the health mark, the week screen's carries its keys instead.
//!
//! **On right alignment.** The artboards place the nav at column 76 and the
//! Today screen's key hints at column 70; both runs end in "? keys", the nav at
//! column 115 and the hints at 116. They are plainly meant to line up — one
//! above the other on rows 0 and 39 of the same screen — so this module
//! right-aligns both to the same column and the vertical alignment holds at
//! every terminal width, rather than reproducing a one-column artefact of one
//! rendering.

use ratatui::text::Span;

use crate::{
    text,
    tui::{grid::Grid, model::Health, theme::Role},
};

/// Which view is showing, and therefore which nav item is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The default — the pane that stays open in a tmux split all day.
    Today,
    /// Seven days, and the threads that run across them.
    Week,
    /// Settings, opened twice a year behind `^,`.
    Settings,
}

/// Where a screen's chrome sits, which is the only thing that differs between
/// the 120-column layout and the 80-column one.
#[derive(Debug, Clone, Copy)]
pub struct Margins {
    /// The column the left-hand runs start at: 2 wide, 1 narrow.
    pub left: u16,
    /// Cells kept clear at the right-hand end, so both right-aligned runs end
    /// at the same column.
    pub right: u16,
}

impl Margins {
    /// The 120-column layout.
    pub const WIDE: Self = Self { left: 2, right: 4 };
    /// The 80-column layout, which pulls the left margin in by one.
    pub const NARROW: Self = Self { left: 1, right: 4 };

    /// The column a right-aligned run ends at, on a grid `width` wide.
    pub fn right_edge(self, width: u16) -> u16 {
        width.saturating_sub(self.right)
    }
}

/// Draw the title rule: `Mooshik  ·  Thursday 27 August  ·  14:22` and the nav.
///
/// `subject` is what Mooshik is showing — a date on the Today screen, "Your
/// week  ·  21-27 August" on the week screen — and is drawn as furniture beside
/// the brand, which is the only bright thing on the rule.
pub fn title(grid: &mut Grid<'_>, margins: Margins, subject: &str, view: View) {
    let separator = text::get("tui.separator");
    grid.run(
        margins.left,
        0,
        [
            Span::styled(text::get("tui.brand"), Role::Strongest.style()),
            Span::styled(format!("{separator}{subject}"), Role::Furniture.style()),
        ],
    );
    nav(grid, margins, view);
}

/// Draw the nav, right-aligned: `Today   The week   ^, settings   ? keys`.
///
/// Settings has no lit nav item of its own — it is a layer over whatever was
/// showing, and its own rule says `Esc back   ^, close` — so [`View::Settings`]
/// lights nothing here and the screen draws its own rule instead.
fn nav(grid: &mut Grid<'_>, margins: Margins, view: View) {
    let gap = text::get("tui.nav_gap");
    let items = [
        (text::get("tui.nav_today"), Some(View::Today)),
        (text::get("tui.nav_week"), Some(View::Week)),
        (text::get("tui.nav_settings"), None),
        (text::get("tui.nav_keys"), None),
    ];

    let width: usize = items
        .iter()
        .map(|(label, _)| label.chars().count())
        .sum::<usize>()
        + gap.chars().count() * (items.len() - 1);
    let start = margins
        .right_edge(grid.width())
        .saturating_sub(u16::try_from(width).unwrap_or(u16::MAX));

    let mut spans = Vec::with_capacity(items.len() * 2);
    for (index, (label, owner)) in items.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(gap, Role::Furniture.style()));
        }
        let role = if owner == Some(view) {
            Role::Accent
        } else {
            Role::Furniture
        };
        spans.push(Span::styled(label, role.style()));
    }
    grid.run(start, 0, spans);
}

/// Draw the bottom rule as the Today screen has it: the health mark and one
/// word, then how much is remembered, with `keys` right-aligned opposite.
///
/// `row` is passed rather than derived because the wide layout leaves row 38
/// blank between the panels and this rule, while the narrow one does not.
///
/// `scope` is passed rather than read off `health` because the two layouts want
/// different forms of it — "214 things remembered, back to 21 August" wide and
/// "214 remembered" narrow — and both are written, not truncated. See
/// [`Health::short_scope`](crate::tui::model::Health::short_scope).
pub fn health_rule(
    grid: &mut Grid<'_>,
    margins: Margins,
    row: u16,
    health: &Health,
    scope: &str,
    keys: &str,
) {
    let separator = text::get("tui.separator");
    // A mark and one word when Mooshik is keeping up. When it is not, the mark
    // drops to a furniture bullet rather than turning red: `1i` reserves red for
    // a refused credential and for leaving a database behind, and being behind
    // on a queue is neither.
    let (mark, mark_role) = if health.well {
        (text::get("tui.health_mark"), Role::Affirm)
    } else {
        (text::get("tui.health_behind"), Role::Furniture)
    };
    grid.run(
        margins.left,
        row,
        [
            Span::styled(mark, mark_role.style()),
            Span::styled(
                format!(" {}{separator}{scope}", health.state),
                Role::Furniture.style(),
            ),
        ],
    );
    keys_rule(grid, margins, row, keys);
}

/// Draw a right-aligned key hint on `row`.
pub fn keys_rule(grid: &mut Grid<'_>, margins: Margins, row: u16, keys: &str) {
    grid.put_ending_at(
        margins.right_edge(grid.width()),
        row,
        keys,
        Role::Furniture.style(),
    );
}

/// Draw a left-aligned run of furniture on `row` — the week screen's keys, and
/// the composer's "Nothing here needs saving".
pub fn note_rule(grid: &mut Grid<'_>, col: u16, row: u16, note: &str) {
    grid.put(col, row, note, Role::Furniture.style());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, style::Style};

    fn drawn(width: u16, height: u16, draw: impl FnOnce(&mut Grid<'_>)) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
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

    /// The title rule as artboard `1a` draws it: the brand bright at the left
    /// margin, everything beside it furniture.
    #[test]
    fn the_title_rule_matches_the_artboard() {
        let buf = drawn(120, 1, |grid| {
            title(
                grid,
                Margins::WIDE,
                "Thursday 27 August  ·  14:22",
                View::Today,
            );
        });
        let row = row_text(&buf, 0);
        assert!(
            row.starts_with("  Mooshik  ·  Thursday 27 August  ·  14:22"),
            "{row:?}"
        );
        assert_eq!(style_at(&buf, 2, 0), Role::Strongest.style());
        assert_eq!(style_at(&buf, 11, 0), Role::Furniture.style());
    }

    /// One nav item is lit at a time, and it is the view being shown.
    #[test]
    fn the_nav_lights_the_view_being_shown() {
        for (view, lit) in [(View::Today, "Today"), (View::Week, "The week")] {
            let buf = drawn(120, 1, |grid| title(grid, Margins::WIDE, "x", view));
            let row = row_text(&buf, 0);
            let at = row.find(lit).expect("the nav item is drawn");
            let col = u16::try_from(at).expect("within the grid");
            assert_eq!(
                style_at(&buf, col, 0),
                Role::Accent.style(),
                "{lit} is not lit for {view:?}"
            );
        }
    }

    /// Settings is a layer over whatever was showing, so it lights nothing.
    #[test]
    fn settings_lights_no_nav_item() {
        let buf = drawn(120, 1, |grid| {
            title(grid, Margins::WIDE, "x", View::Settings)
        });
        for col in 0..120 {
            assert_ne!(
                style_at(&buf, col, 0),
                Role::Accent.style(),
                "column {col} is lit while settings is open"
            );
        }
    }

    /// The nav and the key hints end at the same column, so the two rules line
    /// up vertically — the point of this module's alignment note.
    #[test]
    fn both_right_hand_runs_end_at_the_same_column() {
        for width in [80u16, 100, 120, 200] {
            let buf = drawn(width, 2, |grid| {
                title(grid, Margins::WIDE, "x", View::Today);
                keys_rule(grid, Margins::WIDE, 1, "? keys");
            });
            let end_of = |row: u16| row_text(&buf, row).trim_end().chars().count();
            assert_eq!(end_of(0), end_of(1), "the two rules disagree at {width}");
            assert_eq!(end_of(0), usize::from(width - Margins::WIDE.right));
        }
    }

    /// Keeping up earns the affirming mark; being behind drops to furniture
    /// rather than spending the reserved red.
    #[test]
    fn being_behind_never_turns_red() {
        let behind = Health {
            state: "Catching up".to_owned(),
            scope: "214 things remembered".to_owned(),
            short_scope: "214 remembered".to_owned(),
            well: false,
        };
        let buf = drawn(120, 1, |grid| {
            health_rule(grid, Margins::WIDE, 0, &behind, &behind.scope, "? keys");
        });
        assert_eq!(style_at(&buf, 2, 0), Role::Furniture.style());
        assert_ne!(style_at(&buf, 2, 0), Role::Danger.style());

        let well = Health {
            well: true,
            ..behind
        };
        let buf = drawn(120, 1, |grid| {
            health_rule(grid, Margins::WIDE, 0, &well, &well.scope, "? keys");
        });
        assert_eq!(style_at(&buf, 2, 0), Role::Affirm.style());
        assert_eq!(row_text(&buf, 0).trim_start().chars().next(), Some('✓'));
    }

    /// The narrow layout pulls the left margin in by one, as artboard `1h` does.
    #[test]
    fn the_narrow_layout_pulls_the_left_margin_in() {
        assert_eq!(Margins::NARROW.left, 1);
        assert_eq!(Margins::WIDE.left, 2);
        let buf = drawn(80, 1, |grid| {
            title(grid, Margins::NARROW, "Thu 27 Aug", View::Today);
        });
        assert!(row_text(&buf, 0).starts_with(" Mooshik"));
    }

    /// A grid too narrow for a right-aligned run clips it at the left edge
    /// instead of panicking on the subtraction.
    #[test]
    fn a_tiny_grid_clips_rather_than_panicking() {
        let buf = drawn(6, 1, |grid| {
            keys_rule(grid, Margins::WIDE, 0, "Tab panel · ? keys");
        });
        assert_eq!(row_text(&buf, 0).chars().count(), 6);
    }
}
