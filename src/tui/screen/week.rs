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
/// The narrowest a day column may be and still hold prose, which is the
/// design's own column width.
///
/// `1b` gives a day 17 cells: 15 of interior, 13 of text after the margin, and
/// the artboard's own "Mum called / mid-incident / — not called / back" is what
/// says so — a wrap at 13, with 13 the width at which its longest word does not
/// break. Below about this, [`wrap`] stops breaking on spaces and starts
/// breaking words mid-letter, a path that exists so a 60-character file path
/// still shows its first characters and not a layout anyone should reach by
/// resizing a window.
///
/// **This guard only ever fires on a terminal narrower than one design
/// column.** [`window`] takes `visible = max(1, width / 17)`, so `width /
/// visible` is at least 17 for every `width >= 17`; below 17 there is one
/// column and it is the whole terminal. So a day column is either the design's
/// size or larger, or the terminal itself is smaller than a single day — and in
/// that last case the frame and the date are drawn and the prose is not.
const MIN_COLUMN_CELLS: u16 = DAY_CELLS;

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

/// Cells kept clear at the right of the detail pane's log, where the design
/// leaves four rather than [`RIGHT_MARGIN`]'s two.
///
/// `1b` breaks `09:42  The ring overflowed in / production`. The pane has 36
/// columns past [`DETAIL_TEXT`] and that string is 33 characters, so the shared
/// two-cell margin left 34 and drew it on one line — at the artboard's own
/// width, against the artboard's own break. Four is the widest margin that
/// reproduces every break in the pane: it leaves 32, one short of the 33 that
/// line needs, while still fitting the pane's two longest lines — `Nothing was
/// dropped — writers` and `Cancelled drinks with friends`, both 29 — whole.
/// Three would leave exactly 33 and change nothing; five and upwards break
/// those two as well. Same arithmetic as [`NOTES_MARGIN`], and the same rule:
/// take the widest margin the artboard's breaks allow.
const LOG_MARGIN: u16 = 4;
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
    let shown = window(&workspace.week, width);
    let selected = selected_in(&shown, &workspace.week);
    columns(
        grid,
        &workspace.week,
        shown.clone(),
        selected,
        today_index(workspace),
        Place::new(0, band.top, width, day_rows),
    );
    let offscreen = workspace
        .week
        .days
        .len()
        .min(usize::from(DAYS))
        .saturating_sub(shown.len());

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
        selected.and_then(|index| workspace.week.days.get(index)),
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

/// Draw the week's bottom rule: the keys at the left margin, the scope under the
/// detail pane it belongs to.
///
/// Both runs are left-aligned — the scope sits under the pane it summarises, so
/// the rule reads as a continuation of the two columns above it — and that is
/// exactly why the scope's column has to be clamped. `detail_col` is a
/// proportion of the width and the keys are a fixed 47 characters from the left
/// margin, so below about 80 columns the proportion lands *inside* the keys —
/// and with the offscreen note beside them, well above that — and the two runs
/// overwrote each other: at 80 columns the rule rendered as
/// `H/L a day · J/K a thread · ^1 today ·21-27 August  ·  214 remembered`. The
/// week screen has no narrow variant, so every 80- and 100-column terminal saw
/// it. The scope is pushed right of the keys, and dropped entirely when even
/// that leaves no room — a rule with one complete run says more than two
/// mangled ones.
///
/// `offscreen` is how many day columns the terminal was too narrow to show, said
/// beside the keys the way `1h` says `four more` beside the thread line it
/// truncated. It rides on the left-hand run because that run is anchored at the
/// margin and always has somewhere to start; the scope is anchored under a pane
/// whose column moves with the width, so it is the run that has to give way.
///
/// **The scope takes its short form, unconditionally.** This rule already
/// carries the week label — the scope is drawn as `21-27 August · 214
/// remembered` — so the long form's "back to 21 August" repeats, a few cells to
/// the right, the range it has just finished reading. `1b` draws the long form
/// because `1b`'s own rule is the only place that week is named; here it is
/// named twice in one run. The Today screen still reads the long form, and
/// `demo.toml`'s `scope` is still written that way, because `1a` draws exactly
/// that on a rule with no week label on it. Neither form is a truncation of the
/// other; see [`Health::short_scope`](crate::tui::model::Health::short_scope).
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
    let col = detail_col.max(keys_end.saturating_add(chrome::RULE_GAP));
    // Measured against the chrome's own right edge, not the terminal's last
    // column: every other right-aligned run on every screen stops four cells
    // short, and this was the one run in the app allowed to touch the final one.
    let edge = band.margins.right_edge(width);
    // Joined, because the live workspace has no week label: the rule read
    // " · 214 things remembered" on the primary path.
    let scope = joined(
        &[&workspace.week.label, &workspace.health.short_scope],
        tight,
    );
    if scope.is_empty() {
        return;
    }
    let scope_width = u16::try_from(scope.chars().count()).unwrap_or(u16::MAX);
    if col.saturating_add(scope_width) <= edge {
        chrome::note_rule(grid, col, band.status, &scope);
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

/// The day the detail pane describes: the selection, clamped into the columns
/// the terminal actually drew.
///
/// [`Week::selected`] is not clamped to the seven days this screen draws.
/// `H`/`L` clamp it to `days.len()`, and the model may hold more than seven — so
/// a ten-day week with the ninth day selected left [`window`] unable to contain
/// the selection while a `Week::selected_day` reading the raw index still
/// returned it: the detail pane described a day that was on no column, and no
/// column was framed as the selected one. Clamping here makes the pane and the
/// row of columns two views of the same day again, which is the whole
/// relationship between them.
///
/// A selection outside the *week* is still nothing selected: a fixture that
/// names a day the model does not have is describing no day at all, and the
/// pane draws its frame with no title rather than inventing one. What is
/// clamped is a selection the week has and this width could not show.
fn selected_in(shown: &std::ops::Range<usize>, week: &Week) -> Option<usize> {
    if shown.is_empty() || week.selected >= week.days.len() {
        return None;
    }
    Some(
        week.selected
            .clamp(shown.start, shown.end.saturating_sub(1)),
    )
}

/// Draw the day columns across the top.
///
/// `shown` is [`window`]'s answer and `selected` is [`selected_in`]'s, passed in
/// rather than recomputed so the columns and the detail pane below them cannot
/// disagree about which day is which.
///
/// `today` is where today is in the week, if the week holds it: `1b` draws two
/// bright columns — the selected day and today — and the other five in the fading
/// step. See [`column`].
fn columns(
    grid: &mut Grid<'_>,
    week: &Week,
    shown: std::ops::Range<usize>,
    selected: Option<usize>,
    today: Option<usize>,
    at: Place,
) {
    let count = u16::try_from(shown.len()).unwrap_or(DAYS);
    if count == 0 {
        return;
    }
    // **Uniform, with no remainder redistribution.** `1b` places its seven
    // columns at 17 cells each from column 0, which reaches column 118 and
    // leaves 119 blank; the artboard simply does not spend the leftover. Spreading
    // it a cell at a time filled the row but put six of the seven columns off the
    // artboard's own stride, and the design's stride is what the day header, the
    // marks and the wrap width are all measured against.
    //
    // The leftover is `width % visible`, which is at most `visible - 1` and so at
    // most six cells — under half a column, and bounded rather than growing with
    // the terminal.
    let each = at.w / count;
    for (offset, index) in shown.enumerate() {
        let col = at
            .col
            .saturating_add(u16::try_from(offset).unwrap_or(0).saturating_mul(each));
        column(
            grid,
            &week.days[index],
            selected == Some(index),
            today == Some(index),
            Place::new(col, at.row, each, at.h),
        );
    }
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
        .saturating_sub(LOG_MARGIN);
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
