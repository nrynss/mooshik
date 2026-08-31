//! The same day at 80x24 — artboard `1h`.
//!
//! The design says exactly what survives and what goes: "What survives: the
//! conversation, one line of what keeps coming back, one line of the trickle,
//! the quiet mark. What goes: the day list and the week ribbon (`^2` shows them
//! full-screen). Nothing scrolls sideways."
//!
//! So this is not a reflow of [`today`](super::today) — it is a different set of
//! decisions about the same data. The three right-hand panels do not become
//! narrower; two of them collapse to a single line each and the third is gone,
//! reachable in full on its own screen. A panel squeezed to eleven columns would
//! technically fit and tell the reader nothing.
//!
//! The two collapsed lines are summaries, and both are built here rather than
//! carried in the model: a thread's own sentence and the trickle's own entries
//! are what they are, and how much of them fits on one 80-column row is a
//! property of this layout.
//!
//! **One deliberate divergence from `1h`.** The artboard places the nav
//! right-aligned but sets the two tail hints — the thread line's "four more" and
//! the bottom rule's keys — at fixed columns, padded there with spaces inside a
//! left-aligned run. That lines them up at exactly 80 columns and nowhere else:
//! at 100 the nav would sit against the right margin with the two hints stranded
//! twenty columns short of it. This right-aligns all three against the same
//! margin, so the vertical alignment the artboard is drawing survives a terminal
//! that is not exactly 80 columns — the same argument
//! [`chrome`](super::chrome)'s header makes about the wide layout's two rules.

use crate::{
    text,
    tui::{
        grid::{Grid, Place},
        model::{Thread, Trickle, Workspace},
        theme::Role,
        widget::marks,
        wrap::ellipsised,
    },
};

use super::{chrome, conversation, joined, spelled, Focus};

/// Rows the composer takes here: two rules and the draft, with no room for the
/// reassurance line the wide layout carries.
const COMPOSER_ROWS: u16 = 3;
/// Rows kept below the panels: the thread line, the trickle line, a blank, and
/// the bottom rule.
const TAIL_ROWS: u16 = 4;
/// Column the collapsed thread line's text starts at, past the left margin:
/// seven marks and a space.
///
/// `1h` row 20 starts at column 1 — the narrow left margin — so the marks take
/// columns 1 to 7, the space takes 8, and `The 512 cap` starts at 9. Against
/// `margins.left` of 1 that is an offset of 8, not 9: the extra cell put every
/// narrow thread line a column right of the artboard.
const THREAD_TEXT: u16 = marks::WEEK as u16 + 1;

/// The fewest cells worth giving the collapsed thread line's sentence before the
/// "N more" hint is dropped to make room for it.
///
/// A short word and an ellipsis. Below this the row is a count of threads it
/// does not name, which says less than the sentence alone would.
const SENTENCE_FLOOR: u16 = 6;

/// Draw the narrow screen over the whole of `grid`.
pub fn draw(grid: &mut Grid<'_>, workspace: &Workspace, focus: Focus) {
    let margins = chrome::Margins::NARROW;
    let width = grid.width();
    let height = grid.height();

    // Joined rather than formatted: the live workspace has no clock yet, and an
    // unconditional separator drew "Mooshik  ·    ·  " on the primary path. See
    // `screen::joined`.
    let subject = joined(
        &[&workspace.now.short_date, &workspace.now.time],
        text::get("tui.separator"),
    );
    // The brand is clamped against the nav's own start, so the two runs of this
    // rule cannot splice into each other on a terminal narrower than both.
    let gap = text::get("tui.narrow.nav_gap");
    let items = nav_items();
    // And not at all on a one-row terminal, where it would share the bottom
    // rule's row and the two would splice — see [`Band::title`].
    if height >= 2 {
        chrome::brand(
            grid,
            margins,
            &subject,
            chrome::nav_start(width, margins, gap, &items),
        );
        chrome::nav(grid, margins, gap, &items);
    }

    let status_row = height.saturating_sub(1);
    let trickle_row = height.saturating_sub(3);
    let thread_row = height.saturating_sub(4);
    // Panels take everything above the tail. A terminal too short for a tail at
    // all gives the panels what is left and drops the collapsed lines, rather
    // than drawing them over the conversation.
    let panel_rows = height.saturating_sub(1).saturating_sub(TAIL_ROWS);

    // The composer takes what the band can spare, the same clamp `today::Split`
    // makes. Round four gave it to the wide layout and not to this one, so at
    // three rows the composer reached the status row and the *draft* was
    // composited into it — `214 rememberedgain once the doc is out█`, a sentence
    // nobody wrote, which is the class of fault this whole port keeps closing.
    let composer_rows = COMPOSER_ROWS.min(height.saturating_sub(2));
    let conversation_rows = panel_rows.saturating_sub(composer_rows);
    conversation::panel(
        grid,
        &workspace.person,
        &workspace.conversation,
        focus == Focus::Conversation,
        Place::new(0, 1, width, conversation_rows),
    );
    conversation::composer(
        grid,
        &workspace.conversation,
        focus == Focus::Conversation,
        Place::new(0, 1 + conversation_rows, width, composer_rows),
    );

    if panel_rows > COMPOSER_ROWS {
        thread_line(grid, workspace.threads.as_slice(), margins, thread_row);
        trickle_line(grid, &workspace.trickle, margins, trickle_row);
    }

    chrome::health_rule(
        grid,
        margins,
        status_row,
        &workspace.health,
        &workspace.health.short_scope,
        text::get("tui.hint_narrow"),
    );
}

/// The abbreviated nav's items: `Today  Week`.
///
/// Only the labels and the gap differ from the wide nav, so
/// [`chrome::nav`](super::chrome::nav) draws it — the width sum, the right-edge
/// offset and the span loop used to be copied here verbatim.
///
/// Today is always the lit one: this screen *is* Today, and `^2` leaves it for
/// the week's own full-screen layout. `1h` also printed a `?`, which is gone
/// along with the wide rule's `? keys` — no key opens it.
fn nav_items() -> [(&'static str, Role); 2] {
    [
        (text::get("tui.nav_today"), Role::Accent),
        (text::get("tui.narrow.nav_week"), Role::Furniture),
    ]
}

/// The strongest thread, on one row, with the count of the rest beside it.
///
/// The marks stay: they are seven cells and they carry the whole "every day this
/// week" claim without a word, which is exactly what a single row needs.
///
/// The count is `1h`'s own "four more" without its `^3`: the key is bound to
/// nothing, and the number is worth saying without pretending there is a key that
/// shows it. Spelled, because the artboard spells it — see
/// [`spelled`](super::spelled).
fn thread_line(grid: &mut Grid<'_>, threads: &[Thread], margins: chrome::Margins, row: u16) {
    let Some(thread) = threads.first() else {
        return;
    };

    let more = threads.len().saturating_sub(1);
    let hint = if more > 0 {
        text::get("tui.narrow_more").replace("{count}", &spelled(more))
    } else {
        String::new()
    };
    let hint_width = u16::try_from(hint.chars().count()).unwrap_or(0);
    let right = margins.right_edge(grid.width());

    // The hint is right-aligned and drawn last, so on a very narrow row it used
    // to land on top of the marks — leaving a partial run of them, which is a
    // false statement about how many days the thread spans rather than a
    // truncated one. The marks are this row's whole claim, so the hint gives way
    // instead: it is a keypress the reader can find elsewhere.
    // — and it gives way to the *sentence* as well. At 23 to 25 columns the hint
    // took its nine cells and left the row naming no thread at all, so widening
    // the terminal by one column made the row say less. `1h` calls this row "one
    // line of what keeps coming back", and a count of the other four is worth
    // nothing beside a line that names none of them.
    //
    // So the hint is drawn only when it can have its cells without taking either
    // of theirs. One decision, taken here, rather than a chain of adjustments:
    // the row is three claims competing for one line and they rank marks,
    // sentence, count.
    let text_col = margins.left.saturating_add(THREAD_TEXT);
    let room = right.saturating_sub(text_col).saturating_sub(2);
    let hint = if hint_width > 0
        && right.saturating_sub(hint_width) > text_col
        && room.saturating_sub(hint_width) >= SENTENCE_FLOOR
    {
        hint
    } else {
        String::new()
    };
    let hint_width = u16::try_from(hint.chars().count()).unwrap_or(0);
    let available = room.saturating_sub(hint_width);

    grid.run(margins.left, row, marks::compact(thread.days));
    let sentence = if thread.because.is_empty() {
        thread.summary.clone()
    } else {
        // The tight separator, not the trickle's bullet: `1h` draws "The 512 cap
        // · every day this week", and a bullet between two pieces of prose is
        // prose. `marks`'s own header draws that line — a glyph a reader *looks
        // at* lives in Rust, a mark between words a reader *reads* lives in
        // `en.toml` — and using the notation const here broke it, identical
        // rendering notwithstanding.
        format!(
            "{}{}{}",
            thread.summary,
            text::get("tui.separator_tight"),
            thread.because.text
        )
    };
    // An ellipsis when the sentence was cut, so a row ending "…overflow blocks,
    // never" reads as truncation rather than as an unfinished thought. Shared
    // with the two dependency lists, which had the same fault and no mark; see
    // `wrap::ellipsised`.
    grid.put(
        text_col,
        row,
        &ellipsised(&sentence, available),
        Role::Body.style(),
    );
    if !hint.is_empty() {
        grid.put_ending_at(right, row, &hint, Role::Furniture.style());
    }
}

/// The trickle, joined onto one row: `· Just remembered: a, b, c`.
fn trickle_line(grid: &mut Grid<'_>, trickle: &[Trickle], margins: chrome::Margins, row: u16) {
    if trickle.is_empty() {
        return;
    }
    let available = margins
        .right_edge(grid.width())
        .saturating_sub(margins.left);
    let prefix = text::get("tui.narrow_trickle_prefix");

    // Entries are added only while the whole of one still fits, rather than
    // wrapping the joined string and keeping its first line: that cut mid-list
    // and left the row ending on a dangling comma. The full list is a keypress
    // away, so dropping the tail is honest; advertising it with a comma is not.
    let mut line = String::from(prefix);
    let mut width = u16::try_from(prefix.chars().count()).unwrap_or(u16::MAX);
    for (index, item) in trickle.iter().enumerate() {
        let separator = if index == 0 {
            ""
        } else {
            text::get("tui.narrow.list_separator")
        };
        let addition = format!("{separator}{}", item.text);
        let addition_width = u16::try_from(addition.chars().count()).unwrap_or(u16::MAX);
        if width.saturating_add(addition_width) > available {
            break;
        }
        line.push_str(&addition);
        width = width.saturating_add(addition_width);
    }
    // Nothing but the prefix means not even the freshest entry fits, and a bare
    // "· Just remembered:" says less than an empty row.
    if width > u16::try_from(prefix.chars().count()).unwrap_or(u16::MAX) {
        grid.put(margins.left, row, &line, Role::Furniture.style());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect};

    use crate::tui::model::{Health, Justification, Speaker, Stamp, Turn};

    fn workspace() -> Workspace {
        Workspace {
            person: "Neom".to_owned(),
            now: Stamp {
                long_date: "Thursday 27 August".to_owned(),
                short_date: "Thu 27 Aug".to_owned(),
                time: "14:22".to_owned(),
            },
            threads: (0..5)
                .map(|n| Thread {
                    summary: format!("Thought {n}"),
                    short_summary: None,
                    days: [true; 7],
                    because: Justification::history("every day this week"),
                    leaned_on: Vec::new(),
                })
                .collect(),
            trickle: vec![
                Trickle::new("12km before standup"),
                Trickle::new("the novel"),
                Trickle::new("call Mum back"),
            ],
            conversation: crate::tui::model::Conversation {
                earlier: Some("... earlier today".to_owned()),
                turns: vec![Turn::Said {
                    time: "14:20".to_owned(),
                    speaker: Speaker::Person,
                    text: "Right. And I still need to call Mum back.".to_owned(),
                }],
                composer: crate::tui::model::Composer {
                    draft: "Called Mum. No answer.".to_owned(),
                },
            },
            health: Health {
                state: "Keeping up".to_owned(),
                scope: "214 things remembered, back to 21 August".to_owned(),
                short_scope: "214 remembered".to_owned(),
                well: true,
            },
            ..Workspace::default()
        }
    }

    fn drawn(w: u16, h: u16, workspace: &Workspace) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let area = buf.area;
        let mut grid = Grid::new(&mut buf, area);
        draw(&mut grid, workspace, Focus::Conversation);
        buf
    }

    fn row_text(buf: &Buffer, row: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, row)].symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    fn all_text(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|r| row_text(buf, r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The artboard's geometry: the conversation to row 16, the composer to 19,
    /// then the two collapsed lines, a blank, and the bottom rule.
    #[test]
    fn the_geometry_matches_the_artboard() {
        let buf = drawn(80, 24, &workspace());
        assert_eq!(
            buf[(0, 1)].symbol(),
            "┌",
            "the conversation panel starts on row 1"
        );
        assert_eq!(
            buf[(0, 16)].symbol(),
            "└",
            "the conversation closes on row 16"
        );
        assert_eq!(buf[(0, 17)].symbol(), "┌", "the composer starts on row 17");
        assert_eq!(buf[(0, 19)].symbol(), "└", "the composer closes on row 19");
        assert!(row_text(&buf, 22).trim().is_empty(), "row 22 is the blank");
        assert!(row_text(&buf, 23).contains("Keeping up"));
    }

    /// The two collapsed lines are there, and the panels they came from are not.
    #[test]
    fn the_side_panels_collapse_to_one_line_each() {
        let buf = drawn(80, 24, &workspace());
        let text = all_text(&buf);
        assert!(!text.contains("What keeps coming back"), "a panel survived");
        assert!(
            !text.contains("┌─ Just remembered"),
            "the trickle panel survived"
        );
        assert!(
            text.contains("Thought 0"),
            "the strongest thread is missing"
        );
        assert!(
            text.contains("· Just remembered: "),
            "the collapsed trickle line is missing"
        );
        assert!(text.contains("12km before standup"));
    }

    /// The thread line keeps its day marks — seven cells that carry the whole
    /// claim — and offers the rest of the list as a keypress.
    #[test]
    fn the_thread_line_keeps_its_marks_and_counts_the_rest() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 20);
        assert!(row.starts_with(" ▄▄▄▄▄▄▄"), "{row:?}");
        // `1h` row 20 puts the thread's text at column 9, right after the seven
        // marks and one space. It was drawn at 10.
        assert_eq!(row.chars().nth(9), Some('T'), "{row:?}");
        // The count, spelled as `1h` spells it, with no key attached: `^3` is
        // bound to nothing.
        assert!(row.contains("four more"), "{row:?}");
        assert!(!row.contains("^3"), "an unbound key is advertised: {row:?}");
    }

    /// A clipped thread line says so, rather than ending on a word that reads as
    /// an unfinished thought.
    #[test]
    fn a_clipped_thread_line_is_marked_as_clipped() {
        let mut long = workspace();
        long.threads[0].summary =
            "The ring holds 512 in flight; overflow blocks, never drops".to_owned();
        let buf = drawn(80, 24, &long);
        let row = row_text(&buf, 20);
        assert!(row.contains('…'), "the clip is not marked: {row:?}");

        // A thread that fits is not marked.
        let mut short = workspace();
        short.threads.truncate(1);
        short.threads[0].summary = "Short".to_owned();
        short.threads[0].because = Justification::history("once");
        let buf = drawn(80, 24, &short);
        let row = row_text(&buf, 20);
        assert!(!row.contains('…'), "a line that fits was marked: {row:?}");
    }

    /// Widening the terminal never makes the row say less. The hint used to take
    /// its nine cells from 23 columns up, leaving the row naming no thread at
    /// all until 26 — so three widths in the middle were worse than the one
    /// below them.
    #[test]
    fn a_wider_row_never_names_fewer_threads() {
        let mut named = Vec::new();
        for width in 16..=60u16 {
            let buf = drawn(width, 24, &workspace());
            let row = row_text(&buf, 20);
            // The sentence starts past the marks, at the text column.
            let sentence: String = row
                .chars()
                .skip(usize::from(1 + THREAD_TEXT))
                .collect::<String>()
                .trim()
                .to_owned();
            named.push((
                width,
                !sentence.is_empty() && !sentence.starts_with("four more"),
            ));
        }
        // Once the row names a thread it never stops as the terminal grows.
        let first = named.iter().position(|(_, named)| *named);
        if let Some(first) = first {
            for (width, names) in &named[first..] {
                assert!(
                    names,
                    "the row names no thread at {width} but did when narrower"
                );
            }
        }
    }

    /// The "more" hint gives way to the day marks rather than landing on them.
    ///
    /// It is right-aligned and drawn last, so on a very narrow row it used to
    /// overwrite them — and a partial run of marks is a false statement about
    /// how many days the thread spans, where a missing hint is only a keypress
    /// the reader has to find elsewhere.
    #[test]
    fn the_more_hint_gives_way_to_the_day_marks() {
        for width in 10..=30u16 {
            let buf = drawn(width, 24, &workspace());
            let row = row_text(&buf, 20);
            let marks: String = row.chars().filter(|c| *c == '▄' || *c == '▁').collect();
            assert!(
                marks.is_empty() || marks.chars().count() == 7,
                "a partial run of marks at {width}: {row:?}"
            );
        }
    }

    /// A single thread offers no "more" hint, because there is no more.
    #[test]
    fn one_thread_offers_no_more_hint() {
        let mut workspace = workspace();
        workspace.threads.truncate(1);
        let buf = drawn(80, 24, &workspace);
        assert!(!row_text(&buf, 20).contains("more"));
    }

    /// The collapsed trickle never ends on a dangling separator, at any width.
    #[test]
    fn the_trickle_line_never_ends_on_a_comma() {
        for width in [40u16, 50, 60, 70, 80, 99] {
            let buf = drawn(width, 24, &workspace());
            let row = row_text(&buf, 21).trim_end().to_owned();
            assert!(!row.ends_with(','), "dangling comma at {width}: {row:?}");
            assert!(
                !row.trim_end().ends_with(':'),
                "a bare prefix was drawn at {width}: {row:?}"
            );
        }
    }

    /// A row too narrow for even the freshest entry draws nothing rather than a
    /// prefix with nothing after it.
    #[test]
    fn a_row_too_narrow_for_one_entry_draws_nothing() {
        let buf = drawn(24, 24, &workspace());
        let row = row_text(&buf, 21);
        assert!(row.trim().is_empty(), "{row:?}");
    }

    /// The collapsed thread line joins its two halves with the prose separator,
    /// not with the trickle's notation bullet.
    ///
    /// The two render identically — both are `" · "` — so this is checked at the
    /// source, which is the only place the difference exists. `marks`'s own header
    /// draws the line: a glyph a reader *looks at* lives in Rust, a mark between
    /// words a reader *reads* lives in `en.toml`, so that a locale which separates
    /// prose differently can say so without editing the design's legend.
    #[test]
    fn the_thread_line_joins_prose_with_a_prose_separator() {
        let production = include_str!("narrow.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("a production half");
        assert!(
            !production.contains("TRICKLE_BULLET"),
            "a notation glyph is used as a prose separator"
        );
        assert!(
            production.contains("tui.separator_tight"),
            "the prose separator is not used"
        );

        // And the rendered row still reads as `1h` draws it.
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 20);
        assert!(
            row.contains(&format!(
                "Thought 0{}every day this week",
                text::get("tui.separator_tight")
            )),
            "{row:?}"
        );
    }

    /// The narrow scope is used, not a truncation of the wide one.
    #[test]
    fn the_narrow_scope_is_the_written_short_form() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 23);
        assert!(row.contains("214 remembered"), "{row:?}");
        assert!(!row.contains("back to 21 August"));
    }

    /// This rule promises no key that does nothing here.
    ///
    /// It offered `Tab panel`, and `1h` has one focus — the conversation and the
    /// composer share it, so `Tab` deliberately moves nothing and
    /// `tab_on_the_narrow_layout_keeps_the_conversation_focused` asserts five
    /// presses move nothing. A key that is bound and inert is the same broken
    /// promise as one that is not bound at all, which is the rule that stripped
    /// `Alt-Enter` from the composer and `? keys` from the nav. `J/K a thread` is
    /// absent for the same reason: this screen draws no thread cursor.
    #[test]
    fn the_narrow_rule_promises_no_key_that_does_nothing_here() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 23);
        assert!(!row.contains("Tab"), "{row:?}");
        assert!(!row.contains("J/K"), "{row:?}");
        // What it does promise is bound and does something.
        assert!(row.contains("^2 week"), "{row:?}");
        assert!(row.contains("Esc stops"), "{row:?}");
    }

    /// The nav abbreviates rather than wrapping, and Today is lit.
    #[test]
    fn the_nav_abbreviates() {
        let buf = drawn(80, 24, &workspace());
        let row = row_text(&buf, 0);
        assert!(row.contains("Today  Week"), "{row:?}");
        assert!(!row.contains("The week"));
        // Neither `^, settings` nor `1h`'s bare `?` is bound to anything, so
        // neither is drawn.
        assert!(!row.contains("settings"), "{row:?}");
        assert!(!row.contains('?'), "{row:?}");
    }

    /// Nothing scrolls sideways: a run that overflows a row is clipped at the
    /// right edge and never continues on the next one.
    ///
    /// This used to assert `row_text(..).chars().count() == width`, which
    /// `row_text` returns by construction — it walks `0..buf.area.width` — so it
    /// could not fail. The invariant that *can* fail is wrapping, so it is checked
    /// the way `grid.rs`'s `overflow_clips_instead_of_wrapping` checks it: the row
    /// under a clipped run must be untouched by it.
    #[test]
    fn nothing_runs_past_the_right_edge() {
        for width in [40u16, 60, 80, 100] {
            // A draft long enough to overrun the composer at every width tested.
            let mut workspace = workspace();
            workspace.conversation.composer.draft = "x".repeat(usize::from(width) + 40);
            let buf = drawn(width, 24, &workspace);

            // The draft fills the composer's interior to its last column...
            let draft_row = 18;
            assert_eq!(
                buf[(width - 2, draft_row)].symbol(),
                "x",
                "the draft did not reach the interior's last column at {width}"
            );
            // ...stops at the panel's right rule rather than writing over it...
            assert_eq!(
                buf[(width - 1, draft_row)].symbol(),
                "│",
                "the draft overwrote the panel's right rule at {width}"
            );
            // ...and the row under it holds only what the narrow layout draws
            // there — the composer's bottom rule — with no spill from above.
            let below = row_text(&buf, draft_row + 1);
            assert!(
                below.chars().all(|c| "└┘─".contains(c)),
                "row {} is not a bare rule at {width}: {below:?}",
                draft_row + 1
            );
        }
    }

    /// A terminal too short for the tail drops the collapsed lines rather than
    /// drawing them over the conversation.
    #[test]
    fn a_very_short_terminal_drops_the_tail() {
        let buf = drawn(80, 6, &workspace());
        let text = all_text(&buf);
        assert!(!text.contains("Just remembered"));
        assert!(text.contains("Keeping up"));
    }

    /// Every size draws without panicking.
    #[test]
    fn every_size_draws_without_panicking() {
        let workspace = workspace();
        for (w, h) in [(1u16, 1u16), (6, 2), (20, 8), (80, 24), (80, 40)] {
            let buf = drawn(w, h, &workspace);
            assert_eq!(buf.area.width, w);
        }
    }

    /// An empty workspace draws chrome and empty frames rather than panicking.
    #[test]
    fn an_empty_workspace_draws() {
        let buf = drawn(80, 24, &Workspace::default());
        assert_eq!(buf[(0, 1)].symbol(), "┌");
    }
}
