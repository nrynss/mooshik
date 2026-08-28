//! Day marks and the week ribbon — how strength gets drawn instead of written.
//!
//! `1i` is emphatic that no number reaches the screen: no tier names, no scores,
//! no percentages. So "how often do I come back to this" is a row of seven
//! marks, one a day, and "how full was that day" is the height of one bar. Both
//! are countable if the reader cares, and neither is labelled.
//!
//! There are two mark notations because they answer to different neighbours,
//! and they are not interchangeable:
//!
//! * [`compact`] — `▄▄▁▄▄▄▁`, seven adjacent cells, on the Today panel where
//!   there is no day header to line up with. Its colours are **fixed**: present
//!   days are always the fading step and absent days always absence, because on
//!   that panel the *text* beside them carries the ranking, and marks that also
//!   changed brightness would encode the same thing twice and blur both.
//! * [`aligned`] — `▇   ▇   ·   ▇`, one mark per day *column*, on the week
//!   screen where `1i` requires that "the same marks sit directly under their
//!   day columns, so a thread lines up with the days it belongs to". Here the
//!   present marks do take the row's ranking colour, because on that screen the
//!   mark row and its summary are one line.
//!
//! Absent days are drawn, not skipped — `1i` calls the baseline mark "absence,
//! drawn rather than written". A blank would make the row's shape unreadable
//! and, worse, make a five-day thread look like a three-day one.
//!
//! **Where a glyph lives, and where a word does.** `src/text/en.toml` owns every
//! string a reader reads; this module owns every glyph a reader *looks at*. The
//! line is notation versus prose. `▄` is not English — it is `1i`'s legend, it
//! means the same in every locale, and a translator who changed it would be
//! editing the design rather than the language. So the marks, the affirming
//! tick, the behind-bullet and the trickle's bullet are consts here, and the
//! words beside them ("Keeping up", "Just remembered") are keys there. The
//! separators *between* words are prose and stay in TOML.

use ratatui::{style::Style, text::Span};

use crate::tui::{
    model::{Day, Load, Tone},
    theme::Role,
};

/// The seven days a thread's marks span — the design's week, Friday first.
pub const WEEK: usize = 7;

/// A day the thought came up on, on the Today panel.
const PRESENT: &str = "▄";
/// A day it did not, dropped to the baseline.
const ABSENT: &str = "▁";
/// A day it came up on, on the week screen, where the mark is taller so it
/// reads under a day column.
const PRESENT_TALL: &str = "▇";
/// A day it did not, on the week screen — too narrow for a baseline block.
const ABSENT_DOT: &str = "·";

/// The affirming mark: `1i`'s green "one mark, one word" on the bottom rule.
pub const HEALTH_MARK: &str = "✓";
/// What stands in for it when Mooshik is behind. Furniture, not red — `1i`
/// reserves red for a refused credential and for leaving a database behind, and
/// being behind on a queue is neither.
pub const HEALTH_BEHIND: &str = "·";
/// The bullet the trickle's entries hang from, spaced as artboard `1a` sets it.
pub const TRICKLE_BULLET: &str = " · ";
/// What marks a line that had to be cut. Notation, not prose: it means "there
/// was more" in every locale, and a row ending "…quantum assum" reads as a
/// truncation where the same row without it reads as a short name. See
/// [`wrap::ellipsised`](crate::tui::wrap::ellipsised), which is the only thing
/// that writes it.
pub const ELLIPSIS: &str = "…";

/// Cells between one day column and the next on the week screen: three for
/// "Fri" plus the space after it.
const DAY_STRIDE: usize = 4;
/// Cells between one day's date and the next in the Today panel's ribbon.
const RIBBON_STRIDE: u16 = 6;
/// Where the ribbon's first date sits, relative to the ribbon's own origin.
const RIBBON_INSET: u16 = 2;

/// Seven adjacent marks for the Today panel: `▄▄▁▄▄▄▁`.
///
/// Adjacent days of the same kind coalesce into one span, so a thread that came
/// up every day is a single `▄▄▄▄▄▄▄` rather than seven spans the caller writes
/// one cell at a time. Both colours are fixed here (see this module's header),
/// which is what makes coalescing safe: neighbouring marks can never disagree
/// about anything but present-or-absent. Between one and seven spans come back,
/// and the caller writes them left to right.
pub fn compact(days: [bool; WEEK]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(WEEK);
    let mut run: Option<bool> = None;
    for came_up in days {
        let (glyph, role) = if came_up {
            (PRESENT, Role::Fading)
        } else {
            (ABSENT, Role::Absence)
        };
        match (run, spans.last_mut()) {
            // Same kind as the mark before it: grow that span instead of
            // pushing another one.
            (Some(previous), Some(last)) if previous == came_up => {
                last.content.to_mut().push_str(glyph);
            }
            _ => spans.push(Span::styled(glyph, role.style())),
        }
        run = Some(came_up);
    }
    spans
}

/// Seven marks spaced onto the week screen's day columns: `▇   ▇   ·   ▇`.
///
/// `present` is the row's own ranking style, so the marks and the summary beside
/// them read as one line. Absent days ignore it and stay absence — a faint row's
/// gaps must still be distinguishable from its marks.
pub fn aligned(days: [bool; WEEK], present: Style) -> Vec<Span<'static>> {
    // A `&'static str` rather than `" ".repeat(DAY_STRIDE - 1)`: this runs once
    // per thread per frame, and the gap is a compile-time constant of the
    // stride. `GAP_CELLS` keeps the two from drifting apart silently.
    const GAP: &str = "   ";
    const GAP_CELLS: usize = DAY_STRIDE - 1;
    const _: () = assert!(GAP.len() == GAP_CELLS);

    let mut spans = Vec::with_capacity(WEEK * 2);
    for (index, came_up) in days.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(GAP));
        }
        spans.push(if *came_up {
            Span::styled(PRESENT_TALL, present)
        } else {
            Span::styled(ABSENT_DOT, Role::Absence.style())
        });
    }
    spans
}

/// The column a day's mark or date sits at in the Today panel's ribbon,
/// relative to the ribbon's origin.
pub fn ribbon_column(day: usize) -> u16 {
    // Saturating on both operations, not just the multiply: a saturated product
    // is `u16::MAX`, and adding the inset to that wrapped to 1 — a bar for day
    // 11 000 landing beside the bar for day 0.
    RIBBON_INSET.saturating_add(
        u16::try_from(day)
            .unwrap_or(u16::MAX)
            .saturating_mul(RIBBON_STRIDE),
    )
}

/// The two rows of the Today panel's ribbon: the dates, and a bar a day.
///
/// `today` is the index of the current day, whose date brightens to the
/// strongest step — the spine everything else hangs from is cyan, and the day
/// you are actually in is the exception.
pub struct Ribbon<'a> {
    days: &'a [Day],
    today: usize,
}

impl<'a> Ribbon<'a> {
    /// A ribbon over `days`, with `today` as an index into it.
    pub fn new(days: &'a [Day], today: usize) -> Self {
        Self { days, today }
    }

    /// The dates row: `  21    22    23    24    25    26    27`.
    pub fn dates(&self) -> Vec<(u16, Span<'static>)> {
        self.days
            .iter()
            .enumerate()
            .map(|(index, day)| {
                let role = if index == self.today {
                    Role::Strongest
                } else {
                    Role::Date
                };
                (
                    ribbon_column(index),
                    Span::styled(day.day_of_month.clone(), role.style()),
                )
            })
            .collect()
    }

    /// The bars row: `  ▃     ▄     ▅     ▄     ▃     ▇     ▄`.
    ///
    /// Today's bar takes the strongest step whatever its tone, so the eye lands
    /// on the current day; a hard day keeps its caution colour.
    pub fn bars(&self) -> Vec<(u16, Span<'static>)> {
        self.days
            .iter()
            .enumerate()
            .map(|(index, day)| {
                let role = bar_role(day.load, index == self.today);
                (
                    ribbon_column(index),
                    Span::styled(day.load.glyph().to_string(), role.style()),
                )
            })
            .collect()
    }
}

/// The role a ribbon bar draws in.
///
/// A hard day stays yellow even when it is today: the design allows caution
/// "twice a week at most", and losing it on the one day the reader is looking at
/// would waste it entirely.
fn bar_role(load: Load, is_today: bool) -> Role {
    match (load.tone, is_today) {
        (Tone::Hard, _) => Role::Caution,
        (_, true) => Role::Strongest,
        (Tone::Notable, false) => Role::Body,
        (Tone::Plain, false) => Role::Furniture,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn day(level: u8, tone: Tone, of_month: &str) -> Day {
        Day {
            day_of_month: of_month.to_owned(),
            load: Load::new(level, tone),
            ..Day::default()
        }
    }

    /// The Today panel's notation, exactly as `1i` draws it.
    #[test]
    fn compact_marks_match_the_legend() {
        assert_eq!(text_of(&compact([true; 7])), "▄▄▄▄▄▄▄");
        let five = [true, true, false, true, true, true, false];
        assert_eq!(text_of(&compact(five)), "▄▄▁▄▄▄▁");
    }

    /// Adjacent days of the same kind come back as one span, which is what the
    /// fixed colours buy: a full week is one span, not seven.
    #[test]
    fn compact_marks_coalesce_into_runs() {
        assert_eq!(compact([true; 7]).len(), 1);
        assert_eq!(compact([false; 7]).len(), 1);
        // `▄▄▁▄▄▄▁` — present, absent, present, absent.
        let five = [true, true, false, true, true, true, false];
        let spans = compact(five);
        assert_eq!(spans.len(), 4);
        let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, ["▄▄", "▁", "▄▄▄", "▁"]);
        // Alternating days cannot coalesce at all, and must not lose a mark.
        let alternating = [true, false, true, false, true, false, true];
        assert_eq!(compact(alternating).len(), WEEK);
        assert_eq!(text_of(&compact(alternating)), "▄▁▄▁▄▁▄");
    }

    /// On the Today panel the marks' colours are fixed, so ranking is carried
    /// once — by the text beside them — rather than encoded twice.
    #[test]
    fn compact_marks_do_not_take_a_ranking_colour() {
        // One present mark, one absent, then a five-day run: three spans.
        let spans = compact([true, false, true, true, true, true, true]);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].style, Role::Fading.style());
        assert_eq!(spans[1].style, Role::Absence.style());
        assert_eq!(spans[2].style, Role::Fading.style());
    }

    /// The week screen's notation, and the stride that puts each mark under its
    /// own three-letter day header.
    #[test]
    fn aligned_marks_sit_under_their_day_columns() {
        let style = Role::Body.style();
        assert_eq!(
            text_of(&aligned([true; 7], style)),
            "▇   ▇   ▇   ▇   ▇   ▇   ▇"
        );
        let some = [true, true, false, true, true, true, false];
        assert_eq!(text_of(&aligned(some, style)), "▇   ▇   ·   ▇   ▇   ▇   ·");
        // "  Fri Sat …" puts Fri at column 4 of the header; the mark row starts
        // two columns earlier, so a stride of four lands each mark on its day.
        assert_eq!(DAY_STRIDE, 4);
    }

    /// The present marks take the row's ranking colour on the week screen — but
    /// the gaps never do, or a faint row's absences would be indistinguishable
    /// from its marks.
    #[test]
    fn aligned_marks_rank_but_their_gaps_do_not() {
        let rank = Role::Strongest.style();
        let spans = aligned([true, false, true, true, true, true, true], rank);
        assert_eq!(spans[0].style, rank);
        assert_eq!(spans[2].style, Role::Absence.style());
        assert_ne!(spans[2].style, rank);
    }

    /// Both notations always draw seven days, present or absent — a gap is
    /// drawn, never skipped, so a five-day thread cannot read as a three-day one.
    #[test]
    fn absence_is_drawn_rather_than_skipped() {
        assert_eq!(text_of(&compact([false; 7])).chars().count(), WEEK);
        assert_eq!(
            text_of(&aligned([false; 7], Role::Body.style()))
                .chars()
                .filter(|c| *c == '·')
                .count(),
            WEEK
        );
    }

    /// The ribbon's stride, and the inset that lines the dates up with the bars
    /// underneath them.
    #[test]
    fn the_ribbon_columns_line_dates_up_with_bars() {
        assert_eq!(ribbon_column(0), 2);
        assert_eq!(ribbon_column(6), 2 + 6 * 6);
        // A day index no week could hold saturates rather than wrapping back
        // into the ribbon: the inset must not overflow a saturated product.
        assert_eq!(ribbon_column(usize::MAX), u16::MAX);
        assert!(ribbon_column(60_000) >= ribbon_column(6));
        let days = [day(3, Tone::Plain, "21"), day(4, Tone::Plain, "22")];
        let ribbon = Ribbon::new(&days, 1);
        let dates = ribbon.dates();
        let bars = ribbon.bars();
        for (date, bar) in dates.iter().zip(bars.iter()) {
            assert_eq!(date.0, bar.0, "a date and its bar are in different columns");
        }
    }

    /// Today's date brightens out of the cyan spine, and so does its bar.
    #[test]
    fn today_is_the_exception_to_the_cyan_spine() {
        let days = [day(3, Tone::Plain, "26"), day(4, Tone::Plain, "27")];
        let ribbon = Ribbon::new(&days, 1);
        let dates = ribbon.dates();
        assert_eq!(dates[0].1.style, Role::Date.style());
        assert_eq!(dates[1].1.style, Role::Strongest.style());
        let bars = ribbon.bars();
        assert_eq!(bars[0].1.style, Role::Furniture.style());
        assert_eq!(bars[1].1.style, Role::Strongest.style());
    }

    /// A hard day keeps its caution colour even when it is today: caution is
    /// spent twice a week at most and must not be lost on the day being read.
    #[test]
    fn a_hard_day_stays_yellow_even_when_it_is_today() {
        assert_eq!(bar_role(Load::new(7, Tone::Hard), false), Role::Caution);
        assert_eq!(bar_role(Load::new(7, Tone::Hard), true), Role::Caution);
        assert_eq!(bar_role(Load::new(4, Tone::Plain), true), Role::Strongest);
        assert_eq!(bar_role(Load::new(4, Tone::Notable), false), Role::Body);
    }

    /// The bar glyphs come from the model's own clamped table, so a ribbon can
    /// never draw a hole.
    #[test]
    fn every_bar_draws_something() {
        for level in 0..=9u8 {
            let days = [day(level, Tone::Plain, "01")];
            let bars = Ribbon::new(&days, 0).bars();
            assert!(!bars[0].1.content.is_empty());
            assert_ne!(bars[0].1.content.as_ref(), " ");
        }
    }
}
