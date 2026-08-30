//! The calendar half of the view's tests: day boundaries, the zone they are
//! computed in, and the tables every date is written from.
//!
//! Split from `view_tests.rs` because it needs a **zone**, and its neighbour is
//! written against a fixed offset. That is the whole argument for this file:
//! `+05:45` catches a boundary computed by rounding a timestamp, and cannot
//! catch a boundary computed in the wrong zone at all — a `DateTime<FixedOffset>`
//! has no transitions in it, so collapsing the reader's zone to its current
//! offset is invisible to every test next door. [`Eastern`] has one transition
//! and is a dozen lines, which is cheaper than a timezone database and does not
//! depend on `TZ` surviving whatever the CI runner does to the environment.

use super::tests::{before, figures, zone, Corpus};
use super::*;

use chrono::{FixedOffset, MappedLocalTime, NaiveDateTime};

/// A zone with one daylight-saving transition: US Eastern's 2026 fall-back, at
/// 06:00 UTC on Sunday 1 November, when −04:00 becomes −05:00.
///
/// Hand-rolled rather than pulled from `chrono-tz`, because one transition is
/// all this needs and a timezone database is a dependency with a release
/// cadence. It is a real transition on a real date, which is the property under
/// test.
#[derive(Clone, Copy, Debug)]
struct Eastern;

impl Eastern {
    /// The instant summer time ends, in UTC.
    fn fall_back() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 1)
            .and_then(|date| date.and_hms_opt(6, 0, 0))
            .expect("2026-11-01T06:00 is a datetime")
    }

    fn summer() -> FixedOffset {
        FixedOffset::west_opt(4 * 3600).expect("EDT is -04:00")
    }

    fn winter() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).expect("EST is -05:00")
    }
}

impl TimeZone for Eastern {
    type Offset = FixedOffset;

    fn from_offset(_offset: &FixedOffset) -> Self {
        Self
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<FixedOffset> {
        self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).expect("noon exists"))
    }

    /// Local noon on every day of the test is unambiguous, and the one hour that
    /// is not — 01:00–02:00 on the transition day, which happens twice — is
    /// reported as such rather than silently resolved. Nothing in `view.rs`
    /// converts a local time back to an instant; this exists because the trait
    /// asks for it.
    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<FixedOffset> {
        let boundary = Self::fall_back() - chrono::Duration::hours(5);
        let ambiguous = boundary..(boundary + chrono::Duration::hours(1));
        if ambiguous.contains(local) {
            return MappedLocalTime::Ambiguous(Self::summer(), Self::winter());
        }
        if *local < boundary {
            MappedLocalTime::Single(Self::summer())
        } else {
            MappedLocalTime::Single(Self::winter())
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
        self.offset_from_utc_datetime(&utc.and_hms_opt(12, 0, 0).expect("noon exists"))
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
        if *utc < Self::fall_back() {
            Self::summer()
        } else {
            Self::winter()
        }
    }
}

/// An instant, given in UTC.
fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .expect("an unambiguous UTC instant")
}

/// A day boundary is the reader's own midnight, not UTC's.
///
/// At +05:45 the two instants below are fourteen minutes apart and on different
/// local dates. Anything that bucketed on the UTC date, or that shifted the
/// timestamp by whole hours, puts them on the same day.
#[test]
fn a_day_boundary_is_the_readers_own_midnight() {
    let midnight = zone()
        .with_ymd_and_hms(2026, 8, 27, 0, 0, 0)
        .single()
        .expect("an unambiguous local midnight")
        .with_timezone(&Utc);

    let mut corpus = Corpus::new();
    corpus.turn(
        Some("Yesterday, just"),
        before(0, 0),
        Some(midnight - chrono::Duration::minutes(7)),
    );
    corpus.turn(
        Some("Today, just"),
        before(0, 0),
        Some(midnight + chrono::Duration::minutes(7)),
    );

    let workspace = corpus.view();
    assert_eq!(workspace.week.days[5].entries[0].text, "Yesterday, just");
    assert_eq!(workspace.week.days[6].entries[0].text, "Today, just");
    // And the time drawn beside it is the reader's, not the stored instant's.
    assert_eq!(
        workspace.week.days[6].entries[0].time.as_deref(),
        Some("00:07")
    );
}

/// The reader's zone is a zone, and a zone is not its offset.
///
/// The mutation this exists to catch is one line, of the kind written while
/// simplifying a generic away: `let now = now.with_timezone(&now.offset().fix())`.
/// Every test against a fixed offset agrees with it, because a fixed offset has
/// no transitions to disagree about. Under it, the turn below — half past
/// midnight on Sunday 1 November, in summer time — is read at winter's offset and
/// moves onto Saturday 31 October, which is M9's wrong-day fault in the one
/// module written to prevent it.
#[test]
fn a_zone_is_not_an_offset_across_a_daylight_saving_change() {
    // Tuesday 3 November, local noon: the week on screen is 28 October to
    // 3 November, and the transition is inside it.
    let now = Eastern
        .with_ymd_and_hms(2026, 11, 3, 12, 0, 0)
        .single()
        .expect("an unambiguous local instant");

    let mut corpus = Corpus::new();
    // 00:30 EDT on Sunday 1 November — before the transition, so still -04:00.
    corpus.turn(
        Some("Half past midnight, summer time"),
        utc(2026, 11, 3, 12, 0),
        Some(utc(2026, 11, 1, 4, 30)),
    );
    // The hour that happens twice: 01:30 local, on both sides of the change.
    corpus.turn(
        Some("Half one, the first time"),
        utc(2026, 11, 3, 12, 0),
        Some(utc(2026, 11, 1, 5, 30)),
    );
    corpus.turn(
        Some("Half one, the second time"),
        utc(2026, 11, 3, 12, 0),
        Some(utc(2026, 11, 1, 6, 30)),
    );
    // And the far side of the 25-hour day, which the same offset would move too.
    corpus.turn(
        Some("Late on Saturday"),
        utc(2026, 11, 3, 12, 0),
        Some(utc(2026, 11, 1, 3, 30)),
    );

    let workspace = of_graph(&figures(), &corpus.graph, now);
    let labels: Vec<&str> = workspace
        .week
        .days
        .iter()
        .map(|day| day.short_label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["Wed 28", "Thu 29", "Fri 30", "Sat 31", "Sun 1", "Mon 2", "Tue 3"]
    );

    let sunday: Vec<&str> = workspace.week.days[4]
        .entries
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(
        sunday,
        [
            "Half past midnight, summer time",
            "Half one, the first time",
            "Half one, the second time"
        ],
        "an instant moved off the day the reader's calendar puts it on"
    );
    assert_eq!(
        workspace.week.days[3]
            .entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>(),
        ["Late on Saturday"]
    );
    // Both 01:30s draw 01:30, an hour apart in UTC and on the same local clock.
    let times: Vec<&str> = workspace.week.days[4]
        .entries
        .iter()
        .filter_map(|entry| entry.time.as_deref())
        .collect();
    assert_eq!(times, ["00:30", "01:30", "01:30"]);
}

/// The week's seven dates are distinct, which is what lets `today_index` find
/// today by matching a day of the month.
///
/// Not reachable from any real clock — seven consecutive dates always differ —
/// but the distinctness is an invariant `week_dates` is the sole guarantor of,
/// and its saturating fallback used to break it: at `NaiveDate::MIN` the
/// subtraction underflowed on all six days behind today and the whole week
/// collapsed onto one date, so the ribbon brightened the last column while the
/// panel described the first.
#[test]
fn a_week_at_the_end_of_the_calendar_is_still_seven_distinct_days() {
    let earliest = Utc.from_utc_datetime(
        &NaiveDate::MIN
            .and_hms_opt(0, 0, 0)
            .expect("midnight exists"),
    );
    let dates = week_dates(&earliest);
    let distinct: std::collections::HashSet<NaiveDate> = dates.iter().copied().collect();
    assert_eq!(distinct.len(), WEEK, "{dates:?}");

    let workspace = of_graph(&figures(), &Corpus::new().graph, earliest);
    let days: Vec<&str> = workspace
        .week
        .days
        .iter()
        .map(|day| day.day_of_month.as_str())
        .collect();
    assert_eq!(days.len(), WEEK);
    // The one property `screen::today::today_index` rests on: today matches
    // exactly one column, and it is the last.
    let matching = days
        .iter()
        .filter(|day| **day == workspace.today.day_of_month)
        .count();
    assert_eq!(matching, 1, "{days:?}");
    assert_eq!(days[WEEK - 1], workspace.today.day_of_month);
}

/// Every date the calendar can ask for has a name, in all four tables.
///
/// `weekday` and `month` build their key with `format!` and hand it to
/// `text::get`, which **panics** on a missing key — inside the draw path. The
/// suite otherwise lives in August, September and January, so eight months of
/// the year were reachable only by a user in them. A year of consecutive days is
/// every weekday and every month, through the same call the screens make.
#[test]
fn every_day_of_a_year_has_a_name_in_every_table() {
    let mut date = NaiveDate::from_ymd_opt(2024, 1, 1).expect("a leap year begins");
    let end = NaiveDate::from_ymd_opt(2025, 1, 8).expect("and runs past its end");
    let mut months = std::collections::HashSet::new();
    let mut weekdays = std::collections::HashSet::new();
    while date < end {
        for rendered in [
            long_date(date),
            short_date(date),
            day_head(date),
            day_month(date),
        ] {
            assert!(!rendered.trim().is_empty(), "{date} rendered nothing");
            assert!(
                !rendered.contains('{'),
                "{date} left a placeholder: {rendered}"
            );
        }
        months.insert(date.month());
        weekdays.insert(date.weekday().number_from_monday());
        // And the label for the week ending on this day, which reaches the month
        // tables by a second route when the week crosses one.
        let dates =
            week_dates(&Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).expect("noon exists")));
        assert!(!week_label(dates[0], dates[WEEK - 1]).contains('{'));
        date = date.succ_opt().expect("the year advances");
    }
    assert_eq!(months.len(), 12);
    assert_eq!(weekdays.len(), 7);
}
