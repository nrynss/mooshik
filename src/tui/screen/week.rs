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

use super::{chrome, joined, spelled, Band, RIGHT_MARGIN};

/// Days across the top.
const DAYS: u16 = 7;
/// Rows the day columns take.
const DAY_ROWS: u16 = 15;
/// Cells `1b` gives one day column: seven of them at 17, side by side.
///
/// A day column is not a proportion of the width, it is a fixed size — which is
/// why the row of them is *windowed* rather than narrowed. See [`window`].
const DAY_CELLS: u16 = 17;
/// The narrowest a day column may be and still hold prose.
///
/// `1b` gives it 17 cells: 15 of interior, 13 of text after the margin, and the
/// artboard's own "Mum called / mid-incident / — not called / back" is what says
/// so. Below about this, [`wrap`] stops breaking on spaces and starts breaking
/// words mid-letter — a path that exists so a 60-character path still shows its
/// first characters, not a layout anyone should reach by resizing a window. The
/// window never produces a column this narrow unless the whole terminal is; the
/// guard in [`column`] is what makes that loud rather than silent.
const MIN_COLUMN_CELLS: u16 = 10;

/// Cells kept clear at the right of the thread panel — none, where the design
/// leaves none.
///
/// `1b`'s thread panel has 39 columns available past [`THREAD_TEXT`] and spends
/// all of them: `Three days · Monday, Tuesday, Thursday` is 38 characters on one
/// line, and `Cobalt Lantern retries failed fetches` is 37. [`RIGHT_MARGIN`]'s
/// two cut the available width to 37 and broke the first of those. The panel does
/// not need a margin because the marks column on its left is already 33 cells of
/// air — the row does not read as text against a frame.
const THREADS_MARGIN: u16 = 0;

/// Cells kept clear at the right of the detail pane's trailing notes, where the
/// design leaves five.
///
/// `1b` breaks `You still haven't called him back — / it's come up on two days
/// since.` after the dash, and `You came back to the 512 cap four / times on this
/// day.` after `four`. The pane has 43 columns past [`DETAIL_TIME`]; both of
/// those lines fit one more word at 41, so both reflowed. Five is what the
/// artboard's breaks imply, and the notes are the one run in the pane that is
/// prose rather than a log line — the extra air is what separates them from it.
const NOTES_MARGIN: u16 = 5;
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

    // Joined rather than formatted: the live workspace has no week label yet, and
    // an unconditional separator drew "Mooshik  ·  Your week  ·  " on the primary
    // path. See `screen::joined`.
    let subject = joined(
        &[text::get("tui.week_title"), &workspace.week.label],
        text::get("tui.separator"),
    );
    chrome::title(grid, band.margins, &subject, chrome::View::Week);

    let day_rows = DAY_ROWS.min(band.rows());
    let shown = columns(
        grid,
        &workspace.week,
        today_index(workspace),
        band.top,
        width,
        day_rows,
    );
    let offscreen = workspace
        .week
        .days
        .len()
        .min(usize::from(DAYS))
        .saturating_sub(shown);

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

    bottom_rule(grid, workspace, band, detail_col, width, offscreen);
}

/// Which day of the week is today, if this week contains it.
///
/// `None` rather than a fallback, which is where this differs from
/// [`today::today_index`](super::today): that one falls back to the last day so
/// the ribbon is always marked, and here a fallback would brighten the last
/// column of a week that has already ended and call it today. Two days are bright
/// on `1b` — the selected one and today — and being wrong about which is worse
/// than being silent.
fn today_index(workspace: &Workspace) -> Option<usize> {
    if workspace.today.day_of_month.is_empty() {
        return None;
    }
    workspace
        .week
        .days
        .iter()
        .position(|day| day.day_of_month == workspace.today.day_of_month)
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
///
/// `offscreen` is how many day columns the terminal was too narrow to show, said
/// beside the keys the way `1h` says `four more` beside the thread line it
/// truncated. It rides on the left-hand run because that is the run with room
/// when it matters: the columns are only ever windowed below 119 columns, which
/// is also below where the scope stops fitting.
///
/// The scope prefers its **long** form. The comment here used to claim the long
/// one would run off the edge; at the design's width it starts at column 74 and
/// ends at 110 — `1b` draws exactly that — and it only fails to fit below about
/// 87 columns. So the long form is tried, the written short form is the fallback,
/// and nothing at all is the last resort. Neither is a truncation of the other;
/// see [`Health::short_scope`](crate::tui::model::Health::short_scope).
fn bottom_rule(
    grid: &mut Grid<'_>,
    workspace: &Workspace,
    band: Band,
    detail_col: u16,
    width: u16,
    offscreen: usize,
) {
    let tight = text::get("tui.separator_tight");
    let hint = text::get("tui.hint_week");
    // The note is dropped rather than clipped when the rule has no room for both,
    // which is the same choice this rule makes about the scope below: one complete
    // run says more than two mangled ones. Only a terminal under about 66 columns
    // reaches it, and there the keys are the promise worth keeping whole.
    let with_more = joined(&[hint, &offscreen_note(offscreen)], tight);
    let room = band
        .margins
        .right_edge(width)
        .saturating_sub(band.margins.left);
    let keys = if u16::try_from(with_more.chars().count()).unwrap_or(u16::MAX) <= room {
        with_more
    } else {
        hint.to_owned()
    };
    chrome::note_rule(grid, band.margins.left, band.status, &keys);

    let keys_end = band
        .margins
        .left
        .saturating_add(u16::try_from(keys.chars().count()).unwrap_or(u16::MAX));
    let col = detail_col.max(keys_end.saturating_add(RULE_GAP));
    // Measured against the chrome's own right edge, not the terminal's last
    // column: every other right-aligned run on every screen stops four cells
    // short, and this was the one run in the app allowed to touch the final one.
    let edge = band.margins.right_edge(width);
    for form in [&workspace.health.scope, &workspace.health.short_scope] {
        // Joined, because the live workspace has no week label: the rule read
        // " · 214 things remembered" on the primary path.
        let scope = joined(&[&workspace.week.label, form], tight);
        if scope.is_empty() {
            return;
        }
        let scope_width = u16::try_from(scope.chars().count()).unwrap_or(u16::MAX);
        if col.saturating_add(scope_width) <= edge {
            chrome::note_rule(grid, col, band.status, &scope);
            return;
        }
    }
}

/// How many day columns are off screen, said the way `1h` says `four more`.
///
/// Empty when they all fit, so [`joined`] leaves the separator out too. Two keys
/// rather than one, because English inflects the noun; `en.toml` says why.
fn offscreen_note(offscreen: usize) -> String {
    match offscreen {
        0 => String::new(),
        1 => text::get("tui.week_offscreen_one").to_owned(),
        many => text::get("tui.week_offscreen").replace("{count}", &spelled(many)),
    }
}

/// Which of the week's days a terminal `width` columns wide has room for.
///
/// **The columns are windowed, not narrowed.** `1b` gives a day 17 cells and
/// spends every one of them — "Mum called / mid-incident / — not called / back" is
/// a wrap at 13 — so dividing the width by seven whatever the width was is what
/// turned an 80-column terminal into seven 11-cell columns of shredded words.
/// Showing four whole days instead is the decision `1h` makes about the panels it
/// cannot fit: keep what survives at full size, and say what is missing.
///
/// The window is centred on `week.selected` and then pushed back inside the week,
/// so the selected day is always in it and it never runs off either end. `H`/`L`
/// already move `selected`, which is what makes it scroll: there is no second
/// cursor and nothing else to keep in step.
fn window(week: &Week, width: u16) -> std::ops::Range<usize> {
    let days = week.days.len().min(usize::from(DAYS));
    if days == 0 {
        return 0..0;
    }
    let visible = usize::from((width / DAY_CELLS).max(1)).min(days);
    let start = week
        .selected
        .saturating_sub(visible / 2)
        .min(days - visible);
    start..start + visible
}

/// Draw the day columns across the top, returning how many were drawn.
///
/// `today` is where today is in the week, if the week holds it: `1b` draws two
/// bright columns — the selected day and today — and the other five in the fading
/// step. See [`column`].
fn columns(
    grid: &mut Grid<'_>,
    week: &Week,
    today: Option<usize>,
    row: u16,
    width: u16,
    height: u16,
) -> usize {
    let shown = window(week, width);
    let count = u16::try_from(shown.len()).unwrap_or(DAYS);
    if count == 0 {
        return 0;
    }
    let each = width / count;
    // The remainder is spread one cell at a time rather than dumped on one column,
    // so no column is more than a single cell wider than another — the last column
    // used to take the whole of it and came out six cells wider than its
    // neighbours at 137 columns. The wide ones are the *last* ones, because at the
    // design's 120 the remainder is exactly one: padding the first column instead
    // would shift all seven off the artboard's own 17-cell stride.
    let plain = count.saturating_sub(width % count);
    let mut at = 0;
    for (offset, index) in shown.clone().enumerate() {
        let panel_width = if u16::try_from(offset).unwrap_or(0) < plain {
            each
        } else {
            each.saturating_add(1)
        };
        column(
            grid,
            &week.days[index],
            index == week.selected,
            today == Some(index),
            Place::new(at, row, panel_width, height),
        );
        at = at.saturating_add(panel_width);
    }
    shown.len()
}

/// Draw one day column: the weather, what happened, and how the day felt.
///
/// `selected` and `today` are the two days `1b` brightens. Its seven column text
/// divs are `var(--t2)` — the fading step — for Fri through Tue and `var(--t)` —
/// body — for Wed 26, the selected day, and Thu 27, today. Nothing else about the
/// column changes; the two days the reader came here about simply separate from
/// the five they did not.
fn column(grid: &mut Grid<'_>, day: &Day, selected: bool, today: bool, at: Place) {
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

    // A column too narrow for prose draws its frame and its date and stops — see
    // `MIN_COLUMN_CELLS`. The alternative is words broken mid-letter, which is
    // `wrap`'s last resort for an unbreakable path and not a layout to arrive at
    // by resizing a window.
    if at.w < MIN_COLUMN_CELLS {
        return;
    }

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

    // The two days worth looking at are drawn in body text, the other five in the
    // fading step — `1b`'s `var(--t)` against its `var(--t2)`.
    let plain = if selected || today {
        Role::Body
    } else {
        Role::Fading
    };
    let mut at = 2;
    for entry in &day.highlights {
        let role = match entry.tone {
            Tone::Hard => Role::Caution,
            Tone::Notable => Role::Strongest,
            Tone::Plain => plain,
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
        .saturating_sub(THREADS_MARGIN);
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
            .saturating_sub(NOTES_MARGIN);
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
