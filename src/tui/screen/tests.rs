//! Cross-screen invariants — the rules that hold on every artboard at once.
//!
//! Per-screen geometry is tested beside each screen. What lives here is the
//! design's whole-app claims, the ones a single screen's tests cannot see:
//! nothing paints a background, no colour escapes the sixteen, the double rule
//! appears at most once, and every screen survives every terminal size.

use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::tui::{app::App, grid::Grid, model::Workspace, screen::chrome::View, theme::Role};

/// Every size worth checking: absurdly small, the design's two, and larger.
const SIZES: [(u16, u16); 14] = [
    (1, 1),
    (2, 2),
    (10, 4),
    (40, 12),
    (80, 24),
    (100, 30),
    (120, 40),
    (200, 60),
    (400, 120),
    // Bands too short to hold the panels that assume a fixed height. The
    // composer took its four rows whatever the band was, so at these sizes its
    // frame, its draft and the bottom rule all landed on the same row.
    (120, 2),
    (120, 3),
    (120, 5),
    (100, 6),
    (80, 4),
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

/// Every scene worth sweeping a width range over: the three screens plus the two
/// conversation states, which carry the widest strings in the app — `1d`'s badge
/// is 48 characters of reassurance punched through a card's bottom rule.
fn scenes() -> Vec<(&'static str, App)> {
    let mut all = apps();
    for (name, scene) in [
        ("recall", crate::tui::Scene::Recall),
        ("caution", crate::tui::Scene::Caution),
    ] {
        let mut app = App::new(crate::tui::demo(scene));
        app.view = View::Today;
        all.push((name, app));
    }
    all
}

fn row_text(buf: &Buffer, row: u16) -> String {
    (0..buf.area.width)
        .map(|col| buf[(col, row)].symbol().chars().next().unwrap_or(' '))
        .collect()
}

/// Every width from a small tmux split to a wide one. Stepped by one, because
/// the faults this range exists for are off-by-one collisions that appear over a
/// span of eight or nine columns and are invisible at the round numbers.
// From 16, not 40. Round four found the title rule splicing its brand into the
// nav at 22–29 columns and the narrow thread line's hint landing on top of the
// day marks at 14–21 — both below the documented 80x24 minimum, and both missed
// because this sweep started above them. The week screen has no narrow variant,
// so a small tmux split reaches every one of these widths.
const WIDTHS: std::ops::RangeInclusive<u16> = 16..=200;

/// Heights the two rule-and-frame invariants sweep.
///
/// They used to carry `[24, 40]` and `[12, 24, 40]` of their own, which is why
/// round four's five short `SIZES` bands could not detect the fault they were
/// added for: `SIZES` drives the draws-at-every-size, palette and
/// nothing-outside-the-grid checks, and none of those can see a rule overwritten
/// by a panel. Reverting the composer clamp passed the whole suite.
///
/// The short end is where the faults are, because that is where a panel with a
/// fixed height meets a band that cannot hold it: 2 is the smallest screen that
/// has both a title rule and a bottom rule, 19 is the height at which the week
/// screen's lower panels get one row each, and 1 is the case that used to splice
/// the two rules into each other because the sweep stopped one row above it.
const HEIGHTS: [u16; 15] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 18, 19, 24, 40];

/// **No rule ever writes two of its runs into the same cells.**
///
/// Every rule in the app is two runs written independently from opposite ends,
/// and neither knows how wide the other is. Round two found this on the week's
/// bottom rule and clamped it there; the identical fault was still on the Today
/// and narrow bottom rules — `✓ Keeping up  ·  214 things remembered, back to 21
/// AugusTab panel · …` at every width from 100 to 108, which is the wide
/// layout's own lower bound — and on the title rule of every screen, where the
/// week at 40 drew `  Mooshik  ·  Your wToday   The weekgust`. So it is checked
/// here, on every screen at once, rather than on the screen it happened to be
/// noticed on.
///
/// The check is that the run which must survive is present **whole** and has a
/// space or the row's edge on either side of it. That is exactly the invariant:
/// a splice either destroys the run or leaves it touching the other one, and
/// both are caught. Widths at which the run genuinely cannot fit are skipped —
/// there the rule is a clipped run and not two colliding ones.
#[test]
fn no_rule_writes_two_runs_into_the_same_cells() {
    use crate::tui::{app::NARROW_BELOW, screen::chrome::Margins};

    for (name, mut app) in scenes() {
        for width in WIDTHS {
            let narrow = app.view != View::Week && width < NARROW_BELOW;
            let margins = if narrow {
                Margins::NARROW
            } else {
                Margins::WIDE
            };
            let (nav, hint) = if narrow {
                (
                    format!(
                        "{}{}{}",
                        crate::text::get("tui.nav_today"),
                        crate::text::get("tui.narrow.nav_gap"),
                        crate::text::get("tui.narrow.nav_week")
                    ),
                    crate::text::get("tui.hint_narrow"),
                )
            } else {
                (
                    format!(
                        "{}{}{}",
                        crate::text::get("tui.nav_today"),
                        crate::text::get("tui.nav_gap"),
                        crate::text::get("tui.nav_week")
                    ),
                    if app.view == View::Week {
                        crate::text::get("tui.hint_week")
                    } else {
                        crate::text::get("tui.hint_today")
                    },
                )
            };
            for height in HEIGHTS {
                let buf = draw(&mut app, width, height);
                // One row holds the bottom rule alone — the two would otherwise
                // splice into each other, which is what `Band::title` refuses.
                // So the title rule is checked only where there is one.
                if height >= 2 {
                    whole_and_apart(&buf, 0, &nav, margins, &format!("{name} nav at {width}"));
                }
                whole_and_apart(
                    &buf,
                    height - 1,
                    hint,
                    margins,
                    &format!("{name} hint at {width}"),
                );
            }
        }
    }
}

/// Assert `run` appears whole on `row` with nothing but space beside it.
///
/// Skipped where the rule has no room for it: below that the run is legitimately
/// clipped, and a clipped run is not the fault this is looking for.
fn whole_and_apart(
    buf: &Buffer,
    row: u16,
    run: &str,
    margins: crate::tui::screen::chrome::Margins,
    what: &str,
) {
    let width = buf.area.width;
    let room = margins
        .right_edge(width)
        .saturating_sub(margins.left)
        .saturating_sub(crate::tui::screen::chrome::RULE_GAP);
    if u16::try_from(run.chars().count()).unwrap_or(u16::MAX) > room {
        return;
    }
    let line = row_text(buf, row);
    let at = line
        .find(run)
        .unwrap_or_else(|| panic!("{what}: the run was overwritten — {line:?}"));
    let before = line[..at].chars().next_back();
    assert!(
        before.is_none_or(|c| c == ' '),
        "{what}: another run abuts it — {line:?}"
    );
    let after = line[at + run.len()..].chars().next();
    assert!(
        after.is_none_or(|c| c == ' '),
        "{what}: another run abuts it — {line:?}"
    );
}

/// **No panel writes outside its own frame.**
///
/// A panel's title and badge are punched through its own rules at an inset, and
/// they used to be written straight onto the grid — which clips at the grid and
/// not at the frame. So a title or badge longer than `w - 4` overwrote the
/// panel's right rule and carried on into whatever sat beside it: `--demo
/// caution` lost the recall card's bottom-right corner between 100 and 103
/// columns, the week's detail pane lost its `┐` at 60, and the week's thread
/// panel spilled out of itself at 40. `1i` makes the frame the signal — "A light
/// frame is a panel. Accent frame means focused" — so a missing corner says
/// something false about focus rather than merely looking untidy.
///
/// Checked as corner balance per row: on any row, every `┌`/`└` is closed by a
/// `┐`/`┘` before the next one opens, and none is left open at the end. A title
/// that ate its own right rule takes that corner with it, and a badge that
/// spilled into the panel beside it opens a second frame before the first has
/// closed.
#[test]
fn no_panel_writes_over_its_own_frame() {
    for (name, mut app) in scenes() {
        for width in WIDTHS {
            for height in HEIGHTS {
                let buf = draw(&mut app, width, height);
                for row in 0..height {
                    let mut open = false;
                    for col in 0..width {
                        match buf[(col, row)].symbol() {
                            "┌" | "└" => {
                                assert!(
                                    !open,
                                    "{name} at {width}x{height} row {row}: a frame opens at \
                                     column {col} while another is still open — {:?}",
                                    row_text(&buf, row)
                                );
                                open = true;
                            }
                            "┐" | "┘" => {
                                assert!(
                                    open,
                                    "{name} at {width}x{height} row {row}: a frame closes at \
                                     column {col} with none open — {:?}",
                                    row_text(&buf, row)
                                );
                                open = false;
                            }
                            _ => {}
                        }
                    }
                    assert!(
                        !open,
                        "{name} at {width}x{height} row {row}: a frame never closes — {:?}",
                        row_text(&buf, row)
                    );
                }
            }
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

/// **Every stop the `Tab` cycle offers accents a panel that is on screen.**
///
/// The third class of fault these invariants could not see, and the reason it
/// took seven rounds to find: every other sweep here draws through `scenes()`,
/// which never touches `focus`, so the whole of `WIDTHS × HEIGHTS` ran at
/// `Focus::default()`. The focus dimension was unswept.
///
/// Twice now a cycle has offered a stop the screen was not drawing — first
/// `Focus::Threads` under a standing caution, then `Focus::Trickle` on any
/// terminal 32 rows or shorter — and both times the symptom was the same: the
/// user presses `Tab`, nothing changes, and there is no accent anywhere to say
/// where they are. `1i` gives focus a colour precisely so that question has an
/// answer, so a stop with no accent behind it is a broken screen, not a cosmetic
/// one.
///
/// Widths are sampled rather than swept: the fault is height-driven — it is
/// `Split` dropping panels — and this test costs four draws per size where the
/// others cost one.
#[test]
fn every_cycle_stop_accents_a_panel_on_screen() {
    use crate::tui::app::Action;

    const SAMPLED_WIDTHS: [u16; 5] = [100, 110, 120, 160, 200];

    for (name, mut app) in scenes() {
        for width in SAMPLED_WIDTHS {
            for height in HEIGHTS {
                // The first draw is what tells the cycle the terminal's size.
                let buf = draw(&mut app, width, height);
                if !accents_a_frame(&buf) {
                    // Too small for any panel to draw a frame at all; there is
                    // nothing for focus to land on and nothing to promise.
                    continue;
                }
                // Every stop, all the way round.
                for step in 0..5 {
                    app.apply(Action::NextPanel);
                    let buf = draw(&mut app, width, height);
                    assert!(
                        accents_a_frame(&buf),
                        "{name} at {width}x{height}: {:?} accents no panel on screen \
                         (Tab press {})",
                        app.focus(),
                        step + 1
                    );
                }
            }
        }
    }
}

/// Whether any panel frame on screen is drawn in the accent.
///
/// Box-drawing glyphs only: the nav's lit item and the composer's prompt and
/// cursor are also the accent and are unconditional chrome, so counting every
/// accent cell would call a screen focused when nothing is.
fn accents_a_frame(buf: &Buffer) -> bool {
    buf.content.iter().any(|cell| {
        cell.fg == Role::Accent.color()
            && matches!(
                cell.symbol(),
                "┌" | "┐" | "└" | "┘" | "─" | "│" | "╔" | "╗" | "╚" | "╝" | "═" | "║"
            )
    })
}
