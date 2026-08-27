//! Cross-screen invariants — the rules that hold on every artboard at once.
//!
//! Per-screen geometry is tested beside each screen. What lives here is the
//! design's whole-app claims, the ones a single screen's tests cannot see:
//! nothing paints a background, no colour escapes the sixteen, the double rule
//! appears at most once, and every screen survives every terminal size.

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::tui::{app::App, grid::Grid, model::Workspace, screen::chrome::View, theme::Role};

/// Every size worth checking: absurdly small, the design's two, and larger.
const SIZES: [(u16, u16); 9] = [
    (1, 1),
    (2, 2),
    (10, 4),
    (40, 12),
    (80, 24),
    (100, 30),
    (120, 40),
    (200, 60),
    (400, 120),
];

fn draw(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let area = buf.area;
    let mut grid = Grid::new(&mut buf, area);
    app.draw(&mut grid);
    buf
}

fn apps() -> Vec<(&'static str, App)> {
    let mut today = App::new(crate::tui::demo(crate::tui::Scene::Today));
    today.view = View::Today;
    let mut week = App::new(crate::tui::demo(crate::tui::Scene::Today));
    week.view = View::Week;
    let empty = App::new(Workspace::default());
    vec![("today", today), ("week", week), ("empty", empty)]
}

fn cells(buf: &Buffer) -> impl Iterator<Item = &ratatui::buffer::Cell> {
    buf.content.iter()
}

/// Every screen draws at every size without panicking, and fills exactly the
/// buffer it was given.
#[test]
fn every_screen_draws_at_every_size() {
    for (name, mut app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&mut app, width, height);
            assert_eq!(buf.area.width, width, "{name} at {width}x{height}");
            assert_eq!(buf.area.height, height, "{name} at {width}x{height}");
        }
    }
}

/// Nothing paints a background except the ground behind a punched-through title
/// or badge. `1i`: "Mooshik never paints a background — this is just what is
/// behind."
#[test]
fn nothing_paints_a_background_but_the_ground() {
    for (name, mut app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&mut app, width, height);
            for cell in cells(&buf) {
                assert!(
                    cell.bg == Color::Reset || cell.bg == Role::Ground.color(),
                    "{name} at {width}x{height} paints {:?}",
                    cell.bg
                );
            }
        }
    }
}

/// No colour escapes the sixteen the design commits to — no RGB, no 256-colour
/// index, and none of the five brights held back on purpose.
#[test]
fn no_colour_escapes_the_sixteen() {
    const HELD_BACK: [Color; 5] = [
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightCyan,
    ];
    for (name, mut app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&mut app, width, height);
            for cell in cells(&buf) {
                for colour in [cell.fg, cell.bg] {
                    assert!(
                        !matches!(colour, Color::Rgb(..) | Color::Indexed(_)),
                        "{name} at {width}x{height} uses {colour:?}"
                    );
                    assert!(
                        !HELD_BACK.contains(&colour),
                        "{name} at {width}x{height} spends the held-back {colour:?}"
                    );
                }
            }
        }
    }
}

/// Red is reserved: `1i` gives it two uses in the whole app, neither of which is
/// on the Today or week screens.
#[test]
fn the_reserved_red_is_unspent_on_these_screens() {
    for (name, mut app) in apps() {
        let buf = draw(&mut app, 120, 40);
        for cell in cells(&buf) {
            assert_ne!(
                cell.fg,
                Role::Danger.color(),
                "{name} spends the reserved red"
            );
        }
    }
}

/// The double rule appears exactly once in the app, on the one irreversible
/// action — so it appears nowhere on these screens.
#[test]
fn no_double_rule_on_these_screens() {
    for (name, mut app) in apps() {
        let buf = draw(&mut app, 120, 40);
        for cell in cells(&buf) {
            assert!(
                !matches!(cell.symbol(), "╔" | "╗" | "╚" | "╝" | "═" | "║"),
                "{name} draws a double rule: {:?}",
                cell.symbol()
            );
        }
    }
}

/// The bottom rule is always the last row and always says something, so the
/// screen never ends on a bare panel edge.
#[test]
fn the_bottom_rule_is_always_the_last_row() {
    let mut apps = apps();
    for (name, app) in &mut apps {
        if *name == "empty" {
            continue;
        }
        for (width, height) in [(80u16, 24u16), (120, 40), (200, 60)] {
            let buf = draw(app, width, height);
            let last: String = (0..width)
                .map(|col| {
                    buf[(col, height - 1)]
                        .symbol()
                        .chars()
                        .next()
                        .unwrap_or(' ')
                })
                .collect();
            assert!(
                !last.trim().is_empty(),
                "{name} at {width}x{height} has an empty bottom rule"
            );
        }
    }
}

/// Nothing scrolls sideways: no screen writes a single cell outside the grid it
/// was handed, at any size — which is what makes a 120x40 layout safe to draw in
/// an 80-column terminal.
///
/// This used to assert `buf.content.len() == width * height`, which
/// `Buffer::empty` guarantees and no drawing can change: cells are written
/// through indexing, so the vector's length is fixed and the assertion could not
/// fail. The invariant that *can* fail is escaping the area, so it is checked the
/// way `grid.rs`'s `overflow_clips_instead_of_wrapping` checks it — write into
/// the edges and look at the neighbours. The grid is handed a sub-rectangle of a
/// larger buffer whose every cell starts as a sentinel; a run that ran past the
/// right edge, or a panel placed past the bottom, would leave a mark in the
/// margin around it.
#[test]
fn nothing_is_written_outside_the_grid() {
    /// A symbol no screen draws, so any survivor in the margin is untouched and
    /// any casualty is a write that escaped.
    const SENTINEL: &str = "\u{2591}";
    /// Cells of margin on each side. Two, so a one-column overrun and a
    /// two-column one are both visible.
    const MARGIN: u16 = 2;

    for (name, mut app) in apps() {
        for (width, height) in SIZES {
            let mut buf = Buffer::empty(Rect::new(0, 0, width + MARGIN * 2, height + MARGIN * 2));
            for cell in buf.content.iter_mut() {
                cell.set_symbol(SENTINEL);
            }
            let area = Rect::new(MARGIN, MARGIN, width, height);
            app.draw(&mut Grid::new(&mut buf, area));

            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    let inside = (MARGIN..MARGIN + width).contains(&x)
                        && (MARGIN..MARGIN + height).contains(&y);
                    if inside {
                        continue;
                    }
                    assert_eq!(
                        buf[(x, y)].symbol(),
                        SENTINEL,
                        "{name} at {width}x{height} wrote {:?} outside its grid, \
                         at ({x}, {y})",
                        buf[(x, y)].symbol()
                    );
                }
            }
        }
    }
}

/// Dim only ever accompanies the two colours that use it to double up — a dim
/// accent or a dim yellow would be a fifth ramp step nobody defined.
#[test]
fn dim_is_only_used_to_double_two_slots() {
    use ratatui::style::Modifier;
    for (name, mut app) in apps() {
        let buf = draw(&mut app, 120, 40);
        for cell in cells(&buf) {
            if cell.modifier.contains(Modifier::DIM) {
                assert!(
                    cell.fg == Role::Furniture.color() || cell.fg == Role::Body.color(),
                    "{name} dims {:?}, which is not a doubled slot",
                    cell.fg
                );
            }
        }
    }
}

/// Not an assertion — a way to look at the whole screen.
///
/// `cargo test -- --nocapture eyeball` prints the artboards as the terminal
/// would draw them, which is the only way to catch the faults geometry
/// assertions cannot see: a panel one column out, a line wrapped oddly, a
/// footer floating mid-panel.
///
/// Ignored by default: it asserts nothing and prints 100 lines, which is noise
/// in a normal run. `cargo test -- --ignored --nocapture eyeball` to look.
#[test]
#[ignore = "prints the screens for a human to look at; asserts nothing"]
fn eyeball() {
    /// Print one screen, trimmed at the right.
    fn look(name: &str, app: &mut App, w: u16, h: u16) {
        let buf = draw(app, w, h);
        println!("\n=== {name} {w}x{h} ===");
        for row in 0..h {
            let line: String = (0..w)
                .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
                .collect();
            println!("|{}|", line.trim_end());
        }
    }

    // `apps()` has no "narrow" entry — the narrow layout is the Today screen at
    // a narrow width, not a third app — so the size was always the wide one and
    // the `if name == "narrow"` arm here never fired.
    for (name, mut app) in apps() {
        if name == "empty" {
            continue;
        }
        look(name, &mut app, 120, 40);
    }
    let mut narrow = App::new(crate::tui::demo(crate::tui::Scene::Today));
    narrow.view = View::Today;
    look("narrow", &mut narrow, 80, 24);

    // The two artboards that are states of the conversation rather than screens.
    for (name, scene) in [
        ("recall (1c)", crate::tui::Scene::Recall),
        ("caution (1d)", crate::tui::Scene::Caution),
    ] {
        let mut app = App::new(crate::tui::demo(scene));
        app.view = View::Today;
        look(name, &mut app, 120, 40);
        look(name, &mut app, 80, 24);
    }
    // The week's bottom rule is the one that used to overwrite itself, and it did
    // so only below 108 columns; the day columns are windowed below 119. So the
    // narrow widths get a look too.
    let mut week = App::new(crate::tui::demo(crate::tui::Scene::Today));
    week.view = View::Week;
    for (w, h) in [(80u16, 24u16), (90, 30), (100, 30), (118, 40)] {
        look("week", &mut week, w, h);
    }
}
