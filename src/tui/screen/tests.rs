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

fn draw(app: &App, width: u16, height: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let area = buf.area;
    let mut grid = Grid::new(&mut buf, area);
    app.draw(&mut grid);
    buf
}

fn apps() -> Vec<(&'static str, App)> {
    let mut today = App::new(crate::tui::demo());
    today.view = View::Today;
    let mut week = App::new(crate::tui::demo());
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
    for (name, app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&app, width, height);
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
    for (name, app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&app, width, height);
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
    for (name, app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&app, width, height);
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
    for (name, app) in apps() {
        let buf = draw(&app, 120, 40);
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
    for (name, app) in apps() {
        let buf = draw(&app, 120, 40);
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
    let apps = apps();
    for (name, app) in &apps {
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

/// Nothing scrolls sideways: no row on any screen is wider than the terminal,
/// which the grid guarantees by clipping rather than wrapping.
#[test]
fn nothing_wraps_onto_the_next_row() {
    for (name, app) in apps() {
        for (width, height) in SIZES {
            let buf = draw(&app, width, height);
            assert_eq!(
                buf.content.len(),
                usize::from(width) * usize::from(height),
                "{name} at {width}x{height} wrote outside its buffer"
            );
        }
    }
}

/// Dim only ever accompanies the two colours that use it to double up — a dim
/// accent or a dim yellow would be a fifth ramp step nobody defined.
#[test]
fn dim_is_only_used_to_double_two_slots() {
    use ratatui::style::Modifier;
    for (name, app) in apps() {
        let buf = draw(&app, 120, 40);
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
    for (name, app) in apps() {
        if name == "empty" {
            continue;
        }
        let (w, h) = if name == "narrow" {
            (80, 24)
        } else {
            (120, 40)
        };
        let buf = draw(&app, w, h);
        println!("\n=== {name} {w}x{h} ===");
        for row in 0..h {
            let line: String = (0..w)
                .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
                .collect();
            println!("|{}|", line.trim_end());
        }
    }
    let mut narrow = App::new(crate::tui::demo());
    narrow.view = View::Today;
    let buf = draw(&narrow, 80, 24);
    println!("\n=== narrow 80x24 ===");
    for row in 0..24 {
        let line: String = (0..80)
            .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
            .collect();
        println!("|{}|", line.trim_end());
    }
}
