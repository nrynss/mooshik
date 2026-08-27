//! The week — artboard `1b`.
//!
//! "Seven days, and the threads that run across them — strength reads as
//! brightness, day-marks and order, never a tier."
//!
//! Two things distinguish this screen from the Today panel's version of the same
//! data, and both come straight from `1i`:
//!
//! * The day marks are **aligned under their day columns** rather than packed
//!   adjacent, "so a thread lines up with the days it belongs to". The header
//!   row `Fri Sat Sun …` is what they line up against, which is why it is drawn
//!   here and not in [`marks`](crate::tui::widget::marks).
//! * **Every thread shows its reason**, where the Today panel gives one only to
//!   the thread at the top. There is room here, and this is the screen someone
//!   opens to ask why — so "Five days · you checked it again today" earns its
//!   line.

use ratatui::text::Span;

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Day, Entry, Thread, Tone, Week, Workspace},
        theme::{Role, Strength},
        widget::{marks, Kind, Panel},
        wrap::{wrap, wrap_paragraphs},
    },
};

use super::{chrome, Band, RIGHT_MARGIN};

/// Days across the top.
const DAYS: u16 = 7;
/// Rows the day columns take.
const DAY_ROWS: u16 = 15;
/// The column the detail pane starts at on a 120-column screen, as a proportion.
///
/// `u32`, because the product is what overflows: `width * 74` passes `u16::MAX`
/// at 886 columns, and a *saturating* multiply there inverted the split.
const DETAIL_NUMERATOR: u32 = 74;
const DETAIL_DENOMINATOR: u32 = 120;

/// Column the day header and thread marks start at, inside the thread panel.
/// The header's own two leading spaces put `Fri` at column 3, and the marks at
/// column 4 land in the middle of it.
const DAY_HEADER: u16 = 1;
const THREAD_MARKS: u16 = 4;
/// Column thread text starts at: past seven marks at a stride of four.
const THREAD_TEXT: u16 = 33;

/// Column an entry's time sits at in the detail pane, and its text.
const DETAIL_TIME: u16 = 1;
const DETAIL_TEXT: u16 = 8;

/// Draw the week screen over the whole of `grid`.
pub fn draw(grid: &mut Grid<'_>, workspace: &Workspace, thread_cursor: usize) {
    let band = Band::new(grid.height(), chrome::Margins::WIDE);
    let width = grid.width();

    let subject = format!(
        "{}{}{}",
        text::get("tui.week_title"),
        text::get("tui.separator"),
        workspace.week.label
    );
    chrome::title(grid, band.margins, &subject, chrome::View::Week);

    let day_rows = DAY_ROWS.min(band.rows());
    columns(grid, &workspace.week, band.top, width, day_rows);

    let lower_top = band.top.saturating_add(day_rows);
    let lower_rows = band.rows().saturating_sub(day_rows);
    let detail_col = u16::try_from(u32::from(width) * DETAIL_NUMERATOR / DETAIL_DENOMINATOR)
        .unwrap_or(u16::MAX)
        .max(1);

    threads(
        grid,
        &workspace.threads,
        thread_cursor,
        Place::new(0, lower_top, detail_col, lower_rows),
    );
    detail(
        grid,
        workspace.week.selected_day(),
        Place::new(
            detail_col,
            lower_top,
            width.saturating_sub(detail_col),
            lower_rows,
        ),
    );

    bottom_rule(grid, workspace, band, detail_col, width);
}

/// Cells between the two runs on the week's bottom rule.
///
/// Small, because the runs are already columns apart at the design's width; this
/// is the floor that keeps them apart when the terminal is narrower.
const RULE_GAP: u16 = 2;

/// Draw the week's bottom rule: the keys at the left margin, the scope under the
/// detail pane it belongs to.
///
/// Both runs are left-aligned — the scope sits under the pane it summarises, so
/// the rule reads as a continuation of the two columns above it — and that is
/// exactly why the scope's column has to be clamped. `detail_col` is a
/// proportion of the width, and the keys are a fixed 47 characters, so below
/// about 108 columns the proportion lands *inside* the keys and the two runs
/// overwrote each other: at 80 columns the rule rendered as
/// `H/L a day · J/K a thread · ^1 today ·21-27 August  ·  214 remembered`. The
/// week screen has no narrow variant, so every 80- and 100-column terminal saw
/// it. The scope is pushed right of the keys, and dropped entirely when even
/// that leaves no room — a rule with one complete run says more than two
/// mangled ones.
fn bottom_rule(
    grid: &mut Grid<'_>,
    workspace: &Workspace,
    band: Band,
    detail_col: u16,
    width: u16,
) {
    let keys = text::get("tui.hint_week");
    chrome::note_rule(grid, band.margins.left, band.status, keys);

    // The short scope, because this rule already carries the week's own label
    // and the long form ("…, back to 21 August") would run off the edge.
    let scope = format!(
        "{}{}{}",
        workspace.week.label,
        text::get("tui.separator_tight"),
        workspace.health.short_scope
    );
    let keys_end = band
        .margins
        .left
        .saturating_add(u16::try_from(keys.chars().count()).unwrap_or(u16::MAX));
    let col = detail_col.max(keys_end.saturating_add(RULE_GAP));
    let scope_width = u16::try_from(scope.chars().count()).unwrap_or(u16::MAX);
    if col.saturating_add(scope_width) <= width {
        chrome::note_rule(grid, col, band.status, &scope);
    }
}

/// Draw the seven day columns across the top.
fn columns(grid: &mut Grid<'_>, week: &Week, row: u16, width: u16, height: u16) {
    let each = (width / DAYS).max(1);
    for (index, day) in week.days.iter().take(usize::from(DAYS)).enumerate() {
        let at = u16::try_from(index).unwrap_or(0).saturating_mul(each);
        // The last column takes whatever the division left over, so the row of
        // panels reaches the right edge instead of leaving a gap that reads as a
        // rendering fault.
        let is_last = index + 1 == week.days.len().min(usize::from(DAYS));
        let panel_width = if is_last {
            width.saturating_sub(at)
        } else {
            each
        };
        column(
            grid,
            day,
            index == week.selected,
            Place::new(at, row, panel_width, height),
        );
    }
}

/// Draw one day column: the weather, what happened, and how the day felt.
fn column(grid: &mut Grid<'_>, day: &Day, selected: bool, at: Place) {
    // The title is a date, so it takes the date colour — except on the selected
    // day and on a day whose mood is worth noticing, where it brightens.
    let title_role = if selected || matches!(day.mood.as_ref().map(|m| m.tone), Some(Tone::Notable))
    {
        Role::Strongest
    } else {
        Role::Date
    };
    let mut inner = Panel::new(&day.short_label, Kind::focused_if(selected))
        .titled_as(title_role)
        .draw(grid, at);

    // The margin applies here too, and the artboards prove it: `1b`'s Wednesday
    // column is 15 cells of interior and wraps "Mum called mid-incident — not
    // called back" as "Mum called / mid-incident / — not called / back", which
    // is a wrap at 13, not 15. Without it the same text came out "mid-incident
    // —" against the rule.
    let width = inner.width().saturating_sub(RIGHT_MARGIN);
    let last = inner.height().saturating_sub(1);
    if let Some(weather) = &day.weather {
        inner.put(0, 0, weather, Role::Furniture.style());
    }

    let mut at = 2;
    for entry in &day.highlights {
        let role = match entry.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Fading,
        };
        for line in wrap(&entry.text, width) {
            if at >= last {
                // `return`, not `break`: the column is full, and breaking only
                // ended *this* entry's lines — every remaining highlight was
                // still wrapped and then discarded, an allocation per entry per
                // column per frame. There is also nothing left to draw, so the
                // mood below is skipped too, which is what a full column means.
                return;
            }
            inner.put(0, at, &line, role.style());
            at = at.saturating_add(1);
        }
    }

    if let Some(mood) = &day.mood {
        let role = match mood.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Body,
        };
        let mood_row = at.saturating_add(1).min(last);
        inner.put(0, mood_row, &mood.text, role.style());
    }
}

/// Draw "What keeps coming back" with the marks aligned under their days.
fn threads(grid: &mut Grid<'_>, list: &[Thread], cursor: usize, at: Place) {
    let mut inner = Panel::new(text::get("tui.panel_threads"), Kind::Idle).draw(grid, at);

    inner.put(
        DAY_HEADER,
        0,
        text::get("tui.week_day_header"),
        Role::Furniture.style(),
    );

    let width = inner
        .width()
        .saturating_sub(THREAD_TEXT)
        .saturating_sub(RIGHT_MARGIN);
    let height = inner.height();
    let mut at = 1;

    for (rank, thread) in list.iter().enumerate() {
        // The cursor brightens the row it is on without moving it, so `J`/`K`
        // never reorders a list whose order is its meaning.
        let style = if rank == cursor {
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
                inner.run(THREAD_MARKS, at, marks::aligned(thread.days, style));
                first = false;
            }
            inner.put(THREAD_TEXT, at, &line, style);
            at = at.saturating_add(1);
        }
        if !thread.because.is_empty() {
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
        // A blank row between threads: with every reason drawn, the rows would
        // otherwise run together into one block of text.
        at = at.saturating_add(1);
    }
}

/// Draw the selected day in full: its log, and what Mooshik noticed about it.
fn detail(grid: &mut Grid<'_>, day: Option<&Day>, at: Place) {
    let title = day.map_or("", |d| d.long_label.as_str());
    let mut inner = Panel::new(title, Kind::Focused).draw(grid, at);
    let Some(day) = day else { return };

    let height = inner.height();
    if height == 0 {
        return;
    }

    // The weather and the mood share the head of the pane, the mood keeping its
    // own tone so a hard day is yellow here as it is everywhere else.
    let mut head = Vec::new();
    if let Some(weather) = &day.weather {
        head.push(Span::styled(
            format!("{weather}{}", text::get("tui.separator_tight")),
            Role::Furniture.style(),
        ));
    }
    if let Some(mood) = &day.mood {
        let role = match mood.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => Role::Body,
        };
        head.push(Span::styled(mood.text.clone(), role.style()));
    }
    inner.run(DETAIL_TIME, 0, head);

    let width = inner
        .width()
        .saturating_sub(DETAIL_TEXT)
        .saturating_sub(RIGHT_MARGIN);
    let mut at = 2;
    for entry in day.detail_entries() {
        at = log_entry(&mut inner, &entry, at, width, height);
        if at >= height {
            return;
        }
    }

    if !day.notes.trim().is_empty() {
        at = at.saturating_add(1);
        let notes_width = inner
            .width()
            .saturating_sub(DETAIL_TIME)
            .saturating_sub(RIGHT_MARGIN);
        for line in wrap_paragraphs(&day.notes, notes_width) {
            if at >= height {
                return;
            }
            inner.put(DETAIL_TIME, at, &line, Role::Fading.style());
            at = at.saturating_add(1);
        }
    }
}

/// Draw one logged entry with a hanging indent, returning the row after it.
fn log_entry(grid: &mut Grid<'_>, entry: &Entry, row: u16, width: u16, limit: u16) -> u16 {
    let role = match entry.tone {
        Tone::Hard => Role::Caution,
        Tone::Notable => Role::Strongest,
        Tone::Plain => Role::Body,
    };
    let mut at = row;
    let mut first = true;
    for line in wrap(&entry.text, width) {
        if at >= limit {
            return at;
        }
        if first {
            if let Some(time) = &entry.time {
                grid.put(DETAIL_TIME, at, time, Role::Furniture.style());
            }
            first = false;
        }
        grid.put(DETAIL_TEXT, at, &line, role.style());
        at = at.saturating_add(1);
    }
    at
}

#[cfg(test)]
#[path = "week_tests.rs"]
mod tests;
