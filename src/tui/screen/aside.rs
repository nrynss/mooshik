//! The right-hand column: today, what keeps coming back, just remembered.
//!
//! Three panels, stacked, each answering a different question about the same
//! day — what happened, what you keep returning to, and what Mooshik has picked
//! up in the last little while. Artboard `1d` swaps the middle one for "What
//! leans on this", which is the same panel showing one thread's dependents
//! instead of the whole list.
//!
//! **The ranking is drawn twice, deliberately differently.** A thread's text
//! takes the four-step brightness ramp; its day marks do not (they are always
//! the fading step, with absences in absence). The trickle's text takes a
//! *different* ramp that bottoms out in absence rather than furniture. That is
//! not inconsistency — see [`Strength::trickle_role`]: a thread can climb back
//! up, so it never fades to the colour of an absent day, and a trickle line is
//! on its way out, so it does.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Day, Entry, Thread, Tone, Trickle},
        theme::{Role, Strength},
        widget::{marks, Kind, Panel},
        wrap::wrap,
    },
};

use super::RIGHT_MARGIN;

/// Column the timestamp gutter starts at, inside a panel's interior.
const GUTTER: u16 = 1;
/// Column entry text starts at — the design's `--cw * 82` against a panel at
/// column 72, so the gutter is the same eight cells as the conversation's.
const ENTRY_TEXT: u16 = 9;
/// Rows the ribbon takes: the dates, the bars, and a blank under them.
const RIBBON_ROWS: u16 = 3;
/// Cells kept clear at the right-hand end of the Today panel's footer.
const FOOTER_MARGIN: u16 = 2;

/// Cells kept clear at the right of the two thread panels, where the design
/// leaves four rather than [`RIGHT_MARGIN`]'s two.
///
/// `1a`'s thread panel has 35 columns available past [`THREAD_TEXT`], and the
/// artboard breaks `Every day this week · eight / other notes lean on it` after
/// `eight` — a wrap at 27, not at 32 — and `The Quillstone cache lives on / the
/// NAS at /srv/quillstone` after `on`. Two cells left room for one more word on
/// both, so both reflowed. Four is the margin the artboard's own breaks imply.
const THREAD_MARGIN: u16 = 4;

/// Column a thread's day marks sit at, inside the panel's interior.
const MARKS: u16 = 2;
/// Column a thread's text sits at: the marks, then two cells of gutter.
/// `pub(super)` so the Today screen's cursor test can name the column rather
/// than restating the arithmetic.
pub(super) const THREAD_TEXT: u16 = MARKS + marks::WEEK as u16 + 2;
/// Column the trickle's text sits at, after its bullet.
const TRICKLE_TEXT: u16 = 4;
/// Indent of the dependency list in the "What leans on this" panel.
const LEANING_INDENT: u16 = 3;

/// Draw the Today panel: the week ribbon, the day's log, and how the day is.
pub fn today(
    grid: &mut Grid<'_>,
    day: &Day,
    week_days: &[Day],
    today_index: usize,
    focused: bool,
    at: Place,
) {
    let mut inner =
        Panel::new(text::get("tui.panel_today"), Kind::focused_if(focused)).draw(grid, at);

    let ribbon = marks::Ribbon::new(week_days, today_index);
    for (at, span) in ribbon.dates() {
        inner.run(1 + at, 0, [span]);
    }
    for (at, span) in ribbon.bars() {
        inner.run(1 + at, 1, [span]);
    }

    let last = inner.height().saturating_sub(1);
    let entries_end = entries(&mut inner, &day.entries, RIBBON_ROWS, last);

    // The footer follows the log after a blank row, as artboard `1a` places it,
    // and is pushed no further than the panel's last interior row so a long log
    // cannot write it off the bottom.
    let footer_row = entries_end.saturating_add(1).min(last);
    footer(&mut inner, day, footer_row);
}

/// Draw a timed log from `row`, stopping before `limit`. Returns the row after
/// the last line drawn.
fn entries(grid: &mut Grid<'_>, log: &[Entry], row: u16, limit: u16) -> u16 {
    let width = grid
        .width()
        .saturating_sub(ENTRY_TEXT)
        .saturating_sub(RIGHT_MARGIN);
    let mut at = row;
    for entry in log {
        let role = match entry.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Body,
        };
        let mut first = true;
        for line in wrap(&entry.text, width) {
            if at >= limit {
                return at;
            }
            if first {
                if let Some(time) = &entry.time {
                    grid.put(GUTTER, at, &format!(" {time}"), Role::Furniture.style());
                }
                first = false;
            }
            grid.put(ENTRY_TEXT, at, &line, role.style());
            at = at.saturating_add(1);
        }
    }
    at
}

/// Draw the Today panel's footer: the weather, and how the day is going.
///
/// Both are furniture on this panel — artboard `1a` draws "Clear, 19°" and "A
/// good day so far" in the same colour — with the one exception the palette
/// insists on: a hard day keeps its caution colour wherever it appears.
fn footer(grid: &mut Grid<'_>, day: &Day, row: u16) {
    if let Some(weather) = &day.weather {
        grid.put(GUTTER, row, &format!(" {weather}"), Role::Furniture.style());
    }
    if let Some(mood) = &day.mood {
        let role = match mood.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Body,
            Tone::Plain => Role::Furniture,
        };
        let end = grid.width().saturating_sub(FOOTER_MARGIN);
        grid.put_ending_at(end, row, &mood.text, role.style());
    }
}

/// Draw "What keeps coming back": the threads, strongest first.
///
/// Only the top thread's reason is drawn. Position and brightness already rank
/// the rest, and the artboard gives the explanation to the one thread that has
/// earned it — repeating a reason under every row would be the tier list the
/// design is arguing against.
///
/// `cursor` is where `J`/`K` have left the highlight, and it is drawn exactly as
/// the week screen draws it: the row brightens to [`Role::Strongest`] and does
/// not move, because this list's order is its meaning. It is drawn **only while
/// this panel has focus** — `J`/`K` move the cursor from anywhere, so a bright
/// row on an unfocused panel would claim a cursor the keys are not driving, and
/// on the Today screen it would fight the top thread's own rank colour, which is
/// already the brightest step. This screen used to move the cursor and draw
/// nothing at all: the keys the bottom rule advertised had no visible effect.
pub fn threads(grid: &mut Grid<'_>, list: &[Thread], focused: bool, cursor: usize, at: Place) {
    let mut inner =
        Panel::new(text::get("tui.panel_threads"), Kind::focused_if(focused)).draw(grid, at);

    let width = inner
        .width()
        .saturating_sub(THREAD_TEXT)
        .saturating_sub(THREAD_MARGIN);
    let height = inner.height();
    let mut at = 0;

    for (rank, thread) in list.iter().enumerate() {
        let style = if focused && rank == cursor {
            Role::Strongest.style()
        } else {
            Strength::from_rank(rank).style()
        };
        let mut first = true;
        for line in wrap(&thread.summary, width) {
            if at >= height {
                return;
            }
            if first {
                inner.run(MARKS, at, marks::compact(thread.days));
                first = false;
            }
            inner.put(THREAD_TEXT, at, &line, style);
            at = at.saturating_add(1);
        }
        if rank == 0 && !thread.because.is_empty() {
            let role = if thread.because.returned {
                Role::Returned
            } else {
                Role::Furniture
            };
            for line in wrap(&thread.because.text, width) {
                if at >= height {
                    return;
                }
                inner.put(THREAD_TEXT, at, &line, role.style());
                at = at.saturating_add(1);
            }
        }
    }
}

/// Draw "What leans on this" — artboard `1d`'s replacement for the thread list.
///
/// A caution frame, because it is part of the same statement the conversation is
/// making, and one thread rather than five: the question on screen is what
/// depends on the thing about to change.
pub fn leans_on(grid: &mut Grid<'_>, thread: &Thread, at: Place) {
    let mut inner = Panel::new(text::get("tui.panel_leans"), Kind::Caution).draw(grid, at);

    let width = inner
        .width()
        .saturating_sub(THREAD_TEXT)
        .saturating_sub(THREAD_MARGIN);
    let height = inner.height();
    let mut at = 0;
    let mut first = true;
    for line in wrap(&thread.summary, width) {
        if at >= height {
            return;
        }
        if first {
            inner.run(MARKS, at, marks::compact(thread.days));
            first = false;
        }
        inner.put(THREAD_TEXT, at, &line, Role::Strongest.style());
        at = at.saturating_add(1);
    }

    // A blank row, then the count and the list itself.
    at = at.saturating_add(1);
    if at >= height {
        return;
    }
    // Spelled and capitalised, because `1d` writes " Eight things lean on it:".
    let header = text::get("tui.leans.header")
        .replace("{Count}", &super::spelled_leading(thread.leaned_on.len()));
    inner.put(LEANING_INDENT, at, &header, Role::Furniture.style());
    inner.lines(
        LEANING_INDENT,
        at.saturating_add(1),
        &thread.leaned_on,
        Role::Fading.style(),
    );
}

/// Draw "Just remembered": what Mooshik has picked up, freshest first.
pub fn trickle(grid: &mut Grid<'_>, list: &[Trickle], focused: bool, at: Place) {
    let mut inner =
        Panel::new(text::get("tui.panel_trickle"), Kind::focused_if(focused)).draw(grid, at);

    let bullet = marks::TRICKLE_BULLET;
    let width = inner
        .width()
        .saturating_sub(TRICKLE_TEXT)
        .saturating_sub(RIGHT_MARGIN);
    for (rank, line) in list.iter().enumerate() {
        let at = u16::try_from(rank).unwrap_or(u16::MAX);
        if at >= inner.height() {
            return;
        }
        // Something returning from another day takes the returning colour
        // whatever its position, because that is the only thing blue means.
        let role = if line.returned {
            Role::Returned
        } else {
            Strength::trickle_role(rank)
        };
        // The bullet is furniture, except on the rows that have faded to
        // absence — a bright bullet on a nearly-gone line would fight it.
        let bullet_role = if role == Role::Absence {
            Role::Absence
        } else {
            Role::Furniture
        };
        // One line each: the trickle is a glance, not a read, so a long entry
        // is clipped by the panel rather than pushing the next one down.
        let text = wrap(&line.text, width)
            .into_iter()
            .next()
            .unwrap_or_default();
        inner.run(
            GUTTER,
            at,
            [
                Span::styled(bullet, bullet_role.style()),
                Span::styled(text, role.style()),
            ],
        );
    }
}

#[cfg(test)]
#[path = "aside_tests.rs"]
mod tests;
