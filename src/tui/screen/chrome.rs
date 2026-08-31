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

/// Cells kept clear between the two runs of a rule, so they never abut.
///
/// Every rule in the app is two runs written independently — one from the left
/// margin, one placed against the right — and nothing about either knows how
/// wide the other is. Small, because at the design's own width the runs are
/// columns apart; this is the floor that keeps them apart when the terminal is
/// narrower, and `no_rule_writes_two_runs_into_the_same_cells` is what holds it
/// on every screen at once.
pub const RULE_GAP: u16 = 2;

/// `text`'s width in cells, saturating. Characters rather than columns, for the
/// reason [`Grid::put_ending_at`](crate::tui::grid::Grid::put_ending_at) states.
fn width_of(text: &str) -> u16 {
    u16::try_from(text.chars().count()).unwrap_or(u16::MAX)
}

/// Draw the whole title rule: the brand and `subject` at the left margin, the
/// wide nav right-aligned opposite.
///
/// `subject` is what Mooshik is showing — a date on the Today screen, "Your
/// week  ·  21-27 August" on the week screen — and is drawn as furniture beside
/// the brand, which is the only bright thing on the rule.
pub fn title(grid: &mut Grid<'_>, margins: Margins, subject: &str, view: View) {
    let gap = text::get("tui.nav_gap");
    let items = wide_items(view);
    // The brand first and the nav over it: the nav is the run that says which
    // view is showing, so if the clamp below were ever wrong the damage would
    // land on the subject rather than on the navigation.
    brand(
        grid,
        margins,
        subject,
        nav_start(grid.width(), margins, gap, &items),
    );
    nav(grid, margins, gap, &items);
}

/// The column a nav drawn with [`nav`] starts at.
///
/// Split out so the brand can be clamped against it. Both are derived from the
/// same width sum rather than the caller guessing one from the other.
pub fn nav_start(width: u16, margins: Margins, gap: &str, items: &[(&str, Role)]) -> u16 {
    let run: usize = items
        .iter()
        .map(|(label, _)| label.chars().count())
        .sum::<usize>()
        + gap.chars().count() * items.len().saturating_sub(1);
    margins
        .right_edge(width)
        .saturating_sub(u16::try_from(run).unwrap_or(u16::MAX))
}

/// Draw the left half of the title rule: `Mooshik  ·  Thursday 27 August`.
///
/// Split out from [`title`] because the narrow screen writes the same run with
/// its own subject and its own nav — `1h` abbreviates the nav but not the brand.
///
/// `opposite` is the column the run on the right starts at, and the subject is
/// **dropped** rather than clipped when there is no room for it before that.
/// The two runs used to be written with nothing between them, so a narrow
/// terminal spliced one into the other: the week screen at 40 columns drew
/// `  Mooshik  ·  Your wToday   The weekgust`, and at 60 the two abutted. That
/// is below the documented 80x24 minimum on the Today screen, but the week has
/// no narrow variant at all, so a small tmux split reaches it. Dropping the
/// subject is the same choice [`health_rule`] makes about the scope and
/// [`week::bottom_rule`](super::week) makes about the week label: one complete
/// run says more than two mangled ones, and the brand is the run that names the
/// app.
pub fn brand(grid: &mut Grid<'_>, margins: Margins, subject: &str, opposite: u16) {
    let separator = text::get("tui.separator");
    // The separator only where there is a subject to separate the brand from. The
    // live workspace has no date source yet, and `format!("{separator}{subject}")`
    // opened `mooshik tui` on a trailing bullet with nothing after it.
    let brand = text::get("tui.brand");
    let room = opposite
        .saturating_sub(RULE_GAP)
        .saturating_sub(margins.left);
    let tail = if subject.trim().is_empty() {
        String::new()
    } else {
        format!("{separator}{subject}")
    };
    let tail = if width_of(brand).saturating_add(width_of(&tail)) <= room {
        tail
    } else {
        String::new()
    };
    // The brand goes too when even it will not fit. Dropping the subject was
    // half the fix: below about thirty columns `Mooshik` itself ran into the
    // nav and the rule read `MooshikToday   The week`. One complete run says
    // more than two mangled ones — the same choice the bottom rules make, and
    // the nav is the run that survives here because it says which screen this
    // is, where the brand only says which program.
    let brand = if width_of(brand) <= room { brand } else { "" };
    grid.run(
        margins.left,
        0,
        [
            Span::styled(brand, Role::Strongest.style()),
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
    let start = nav_start(grid.width(), margins, gap, items);
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
///
/// **The left run gives way, the keys never do.** The two runs are written from
/// opposite ends and neither knows how wide the other is, so this rule used to
/// overwrite itself exactly as `week::bottom_rule` did before it was clamped —
/// found on the week screen in round two and fixed only there. With
/// `demo.toml`'s scope the left run ends at column 59 and the hints start at
/// `width - 49`, so every terminal from 100 to 108 columns drew
/// `✓ Keeping up  ·  214 things remembered, back to 21 AugusTab panel · …` —
/// and 100 is [`NARROW_BELOW`](crate::tui::app::NARROW_BELOW), the wide
/// layout's own lower bound, so this is the screen the user leaves open all day
/// showing a mid-word splice. The narrow layout reached it below about 67.
///
/// So the scope is dropped first and the state after it: "Keeping up" beside
/// the mark is still `1i`'s "one mark, one word", while a count with no state
/// is a number nobody asked about. The keys are what a hint promises, and a
/// promise half-drawn is worse than one that fits.
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
    keys_rule(grid, margins, row, keys);

    // What is left of the rule once the keys and the gap have taken their cells.
    let room = margins
        .right_edge(grid.width())
        .saturating_sub(width_of(keys))
        .saturating_sub(RULE_GAP)
        .saturating_sub(margins.left);
    // The mark and the space after it come before any word, so a rule with room
    // for neither draws nothing rather than a bare tick against the keys.
    let Some(for_words) = room.checked_sub(MARK_CELLS) else {
        return;
    };
    // Joined, for the reason `brand` joins: a live workspace with no scope yet
    // must not draw the bullet that would have separated it from the state.
    let both = super::joined(&[&health.state, scope], separator);
    let words = [both.as_str(), health.state.as_str(), ""]
        .into_iter()
        .find(|form| width_of(form) <= for_words)
        // Unreachable — the empty form fits any width — but `find` cannot know
        // that, and an `expect` here would be a panic on a drawing path.
        .unwrap_or("");
    grid.run(
        margins.left,
        row,
        [
            Span::styled(mark, mark_role.style()),
            Span::styled(format!(" {words}"), Role::Furniture.style()),
        ],
    );
}

/// Cells the health mark and the space after it take, before a word of state.
const MARK_CELLS: u16 = 2;

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

    /// Below about thirty columns even the brand will not fit before the nav, so
    /// it goes too. Dropping only the subject left the rule reading
    /// `MooshikToday   The week` — two mangled runs where one complete one says
    /// more, and the nav is the run that survives because it says which screen
    /// this is where the brand only says which program.
    #[test]
    fn a_title_rule_with_no_room_drops_the_brand_too() {
        for width in 16..=32u16 {
            let buf = drawn(width, 1, |grid| {
                title(grid, Margins::WIDE, "Thursday 27 August", View::Today);
            });
            let row = row_text(&buf, 0);
            let nav = row.find("Today").map(|byte| row[..byte].chars().count());
            let Some(nav) = nav else { continue };
            // Whatever is drawn to the left of the nav ends before it, with at
            // least one clear cell — never running straight into it.
            let left: String = row.chars().take(nav).collect();
            assert!(
                left.trim().is_empty() || left.ends_with(' '),
                "the title rule splices at {width}: {row:?}"
            );
            assert!(
                !left.contains("Mooshi") || left.contains("Mooshik"),
                "the brand is drawn cut in half at {width}: {row:?}"
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

    /// A title rule with no room for both runs drops the subject and keeps the
    /// brand and the nav whole.
    ///
    /// The two used to be written with nothing between them, so the week screen
    /// at 40 columns drew `  Mooshik  ·  Your wToday   The weekgust` and at 60
    /// they abutted with no space. The brand names the app and the nav says which
    /// view is showing; the subject is the one of the three that repeats
    /// something the screen already shows.
    #[test]
    fn a_narrow_title_rule_drops_the_subject_rather_than_splicing_it() {
        let nav = format!(
            "{}{}{}",
            text::get("tui.nav_today"),
            text::get("tui.nav_gap"),
            text::get("tui.nav_week")
        );
        for width in [40u16, 50, 60, 70] {
            let buf = drawn(width, 1, |grid| {
                title(
                    grid,
                    Margins::WIDE,
                    "Your week  ·  21-27 August",
                    View::Week,
                );
            });
            let row = row_text(&buf, 0);
            assert!(row.contains("Mooshik"), "the brand was lost at {width}");
            let at = row
                .find(&nav)
                .unwrap_or_else(|| panic!("the nav was overwritten at {width}: {row:?}"));
            assert_eq!(
                row[..at].chars().next_back(),
                Some(' '),
                "the two runs abut at {width}: {row:?}"
            );
        }
        // And where there is room, the subject is there in full.
        let buf = drawn(120, 1, |grid| {
            title(
                grid,
                Margins::WIDE,
                "Your week  ·  21-27 August",
                View::Week,
            );
        });
        assert!(row_text(&buf, 0).contains("Your week  ·  21-27 August"));
    }

    /// A bottom rule with no room for both runs drops the scope, then the state,
    /// and never writes either into the keys.
    ///
    /// With `demo.toml`'s scope the left run ends at column 59 and the hint
    /// starts at `width - 49`, so every terminal from 100 to 108 columns drew
    /// `…back to 21 AugusTab panel · …`. 100 is the wide layout's own lower
    /// bound, so this was the screen the user leaves open all day.
    #[test]
    fn a_narrow_bottom_rule_drops_the_scope_rather_than_splicing_it() {
        let health = Health {
            state: "Keeping up".to_owned(),
            scope: "214 things remembered, back to 21 August".to_owned(),
            short_scope: "214 remembered".to_owned(),
            well: true,
        };
        let keys = crate::text::get("tui.hint_today");
        for width in [70u16, 90, 100, 104, 108, 109, 120] {
            let buf = drawn(width, 1, |grid| {
                health_rule(grid, Margins::WIDE, 0, &health, &health.scope, keys);
            });
            let row = row_text(&buf, 0);
            let at = row
                .find(keys)
                .unwrap_or_else(|| panic!("the keys were overwritten at {width}: {row:?}"));
            assert_eq!(
                row[..at].chars().next_back(),
                Some(' '),
                "the two runs abut at {width}: {row:?}"
            );
            assert!(row.contains('✓'), "the mark was lost at {width}: {row:?}");
            // The state survives wherever the mark does; the scope is what goes.
            assert!(row.contains("Keeping up"), "{row:?}");
        }
        // At the design's width the whole scope is there — `1a` draws exactly it.
        let buf = drawn(120, 1, |grid| {
            health_rule(grid, Margins::WIDE, 0, &health, &health.scope, keys);
        });
        assert!(row_text(&buf, 0).contains("back to 21 August"));
        // At 100 it is not, and nothing of it is spliced into the keys either.
        let buf = drawn(100, 1, |grid| {
            health_rule(grid, Margins::WIDE, 0, &health, &health.scope, keys);
        });
        let row = row_text(&buf, 0);
        assert!(!row.contains("214"), "{row:?}");
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
