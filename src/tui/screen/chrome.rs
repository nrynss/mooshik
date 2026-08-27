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
    tui::{grid::Grid, model::Health, theme::Role, widget::marks},
};

/// Which view is showing, and therefore which nav item is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The default — the pane that stays open in a tmux split all day.
    Today,
    /// Seven days, and the threads that run across them.
    Week,
    /// Settings, opened twice a year behind `^,`.
    ///
    /// Nothing reaches it yet — no key is bound to it and no screen draws it;
    /// [`crate::tui::app::App::draw`] falls through to the Today screen. It is
    /// kept because it is the third view the design has, and because the nav's
    /// behaviour for it is already decided and already pinned: settings is a
    /// layer over whatever was showing, so it lights no nav item. See
    /// `settings_lights_no_nav_item`.
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

/// Draw the whole title rule: the brand and `subject` at the left margin, the
/// wide nav right-aligned opposite.
///
/// `subject` is what Mooshik is showing — a date on the Today screen, "Your
/// week  ·  21-27 August" on the week screen — and is drawn as furniture beside
/// the brand, which is the only bright thing on the rule.
pub fn title(grid: &mut Grid<'_>, margins: Margins, subject: &str, view: View) {
    brand(grid, margins, subject);
    nav(grid, margins, text::get("tui.nav_gap"), &wide_items(view));
}

/// Draw the left half of the title rule: `Mooshik  ·  Thursday 27 August`.
///
/// Split out from [`title`] because the narrow screen writes the same run with
/// its own subject and its own nav — `1h` abbreviates the nav but not the brand.
pub fn brand(grid: &mut Grid<'_>, margins: Margins, subject: &str) {
    let separator = text::get("tui.separator");
    // The separator only where there is a subject to separate the brand from. The
    // live workspace has no date source yet, and `format!("{separator}{subject}")`
    // opened `mooshik tui` on a trailing bullet with nothing after it.
    let tail = if subject.trim().is_empty() {
        String::new()
    } else {
        format!("{separator}{subject}")
    };
    grid.run(
        margins.left,
        0,
        [
            Span::styled(text::get("tui.brand"), Role::Strongest.style()),
            Span::styled(tail, Role::Furniture.style()),
        ],
    );
}

/// The wide nav's items: `Today   The week`.
///
/// The design's rule also printed `^, settings` and `? keys`, and both are gone:
/// neither key is bound, and a nav item is a promise the same way a key hint is.
/// They come back with the screens that answer them.
///
/// Settings has no lit item of its own — it is a layer over whatever was
/// showing, and its own rule says `Esc back   ^, close` — so [`View::Settings`]
/// lights nothing and the screen draws its own rule instead.
fn wide_items(view: View) -> [(&'static str, Role); 2] {
    let role_for = |owner: View| {
        if owner == view {
            Role::Accent
        } else {
            Role::Furniture
        }
    };
    [
        (text::get("tui.nav_today"), role_for(View::Today)),
        (text::get("tui.nav_week"), role_for(View::Week)),
    ]
}

/// Draw a nav, right-aligned, its items separated by `gap`.
///
/// The items and the gap are parameters because they are the *only* thing the
/// two navs differ in: `1h`'s is `Today  Week` where `1a`'s is `Today   The
/// week`, and the width sum, the right-edge offset and the span loop were
/// duplicated verbatim in the narrow screen until they were not.
pub fn nav(grid: &mut Grid<'_>, margins: Margins, gap: &str, items: &[(&str, Role)]) {
    let width: usize = items
        .iter()
        .map(|(label, _)| label.chars().count())
        .sum::<usize>()
        + gap.chars().count() * items.len().saturating_sub(1);
    let start = margins
        .right_edge(grid.width())
        .saturating_sub(u16::try_from(width).unwrap_or(u16::MAX));

    let mut spans = Vec::with_capacity(items.len() * 2);
    for (index, (label, role)) in items.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(gap, Role::Furniture.style()));
        }
        spans.push(Span::styled(*label, role.style()));
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
        (marks::HEALTH_MARK, Role::Affirm)
    } else {
        (marks::HEALTH_BEHIND, Role::Furniture)
    };
    // Joined, for the reason `brand` joins: a live workspace with no scope yet
    // must not draw the bullet that would have separated it from the state.
    let words = super::joined(&[&health.state, scope], separator);
    grid.run(
        margins.left,
        row,
        [
            Span::styled(mark, mark_role.style()),
            Span::styled(format!(" {words}"), Role::Furniture.style()),
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

    /// The character column `needle` starts at on `row`.
    ///
    /// `str::find` returns a byte offset, and this rule carries `·` — two bytes
    /// — so the byte offset is one more than the column by the time the nav is
    /// reached, and the assertion below was checking the cell to the right of
    /// the label. Counting characters up to the match is the column.
    fn col_of(buf: &Buffer, row: u16, needle: &str) -> u16 {
        let line = row_text(buf, row);
        let byte = line
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} is not on row {row}"));
        u16::try_from(line[..byte].chars().count()).expect("within the grid")
    }

    /// One nav item is lit at a time, and it is the view being shown.
    #[test]
    fn the_nav_lights_the_view_being_shown() {
        for (view, lit) in [(View::Today, "Today"), (View::Week, "The week")] {
            let buf = drawn(120, 1, |grid| title(grid, Margins::WIDE, "x", view));
            let col = col_of(&buf, 0, lit);
            assert_eq!(
                style_at(&buf, col, 0),
                Role::Accent.style(),
                "{lit} is not lit for {view:?}"
            );
            // And the cell one column right of the label's start — where the
            // byte offset used to land — is still part of the same label.
            assert_eq!(style_at(&buf, col + 1, 0), Role::Accent.style());
        }
    }

    /// Settings is a layer over whatever was showing, so it lights nothing.
    ///
    /// Nothing reaches [`View::Settings`] today — no key opens it and no screen
    /// draws it. This pins the behaviour for a view that becomes reachable only
    /// once the settings screen lands, so the decision is recorded in a test
    /// rather than rediscovered then.
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

    /// Only bound keys reach the nav. `^, settings` and `? keys` were drawn by
    /// the artboards and bound to nothing, so they are not drawn here.
    #[test]
    fn the_nav_advertises_no_unbound_key() {
        let buf = drawn(120, 1, |grid| title(grid, Margins::WIDE, "x", View::Today));
        let row = row_text(&buf, 0);
        assert!(row.contains("Today"), "{row:?}");
        assert!(row.contains("The week"), "{row:?}");
        assert!(!row.contains("settings"), "{row:?}");
        assert!(!row.contains("keys"), "{row:?}");
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
