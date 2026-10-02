//! Turning a caller's two dates into an instant window.
//!
//! Separate from the handler, and taking `now` as an argument, so that the
//! whole of it can be tested without a database, a router or a clock. The
//! handler's job is to map [`WindowError`] onto a status code; the arithmetic
//! is here.
//!
//! Dates rather than instants in the query, and resolved in the space's zone:
//! the boundaries of "October" depend on the zone, so a client computing them
//! would have to know the space's zone and get the daylight-saving arithmetic
//! right to agree with the parser that produced the events.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

/// Why a window could not be read. Each maps to a 400 with this text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowError {
    /// A date that is not `YYYY-MM-DD`.
    Unparseable,
    /// `to` before `from`, which would ask for a negative window.
    Inverted,
}

impl WindowError {
    pub fn message(self) -> &'static str {
        match self {
            WindowError::Unparseable => "dates must be YYYY-MM-DD",
            WindowError::Inverted => "`to` must be after `from`",
        }
    }
}

/// Resolve the query's dates, or `None` when the caller named neither and
/// wants the configured list instead.
///
/// A half-open window is accepted: `from` alone runs to the lookahead, `to`
/// alone starts now.
pub fn bounds(
    from: Option<&str>,
    to: Option<&str>,
    tz: Tz,
    lookahead_days: i64,
    now: DateTime<Utc>,
) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>, WindowError> {
    if from.is_none() && to.is_none() {
        return Ok(None);
    }

    let start = match from {
        Some(text) => start_of_day(text, tz)?,
        None => now,
    };
    let end = match to {
        // Inclusive: a request for 1..31 October means the whole of the 31st,
        // so the exclusive bound handed to the parser is the next midnight.
        Some(text) => start_of_day(text, tz)? + Duration::days(1),
        None => now + Duration::days(lookahead_days),
    };

    if end <= start {
        return Err(WindowError::Inverted);
    }
    Ok(Some((start, end)))
}

/// Local midnight on a `YYYY-MM-DD` date.
fn start_of_day(text: &str, tz: Tz) -> Result<DateTime<Utc>, WindowError> {
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| WindowError::Unparseable)?;
    let naive = date.and_hms_opt(0, 0, 0).ok_or(WindowError::Unparseable)?;
    // Midnight can be skipped by a daylight-saving change -- it is in Lord
    // Howe, and has been in Havana -- so "the first instant of this date"
    // needs a fallback rather than an unwrap.
    Ok(tz
        .from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| (naive + Duration::hours(1)).and_utc()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NY: Tz = chrono_tz::America::New_York;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const NOW: &str = "2026-10-14T22:00:00Z";

    #[test]
    fn no_dates_means_the_configured_list() {
        assert_eq!(bounds(None, None, NY, 30, utc(NOW)), Ok(None));
    }

    #[test]
    fn a_month_runs_from_local_midnight_to_local_midnight() {
        let (from, to) = bounds(Some("2026-10-01"), Some("2026-10-31"), NY, 30, utc(NOW))
            .expect("parses")
            .expect("a window");
        // 1 October in New York begins at 04:00Z, not 00:00Z. The difference
        // is four hours of events on the last evening of September.
        assert_eq!(from, utc("2026-10-01T04:00:00Z"));
        // And the whole of the 31st is included: the bound is the next
        // midnight, so an event at 10pm on the 31st is inside it.
        assert_eq!(to, utc("2026-11-01T04:00:00Z"));
    }

    #[test]
    fn the_window_crosses_a_daylight_saving_change_correctly() {
        // November 2026: the clocks go back on the 1st, so the month's first
        // midnight is EDT (04:00Z) and its last is EST (05:00Z).
        let (from, to) = bounds(Some("2026-11-01"), Some("2026-11-30"), NY, 30, utc(NOW))
            .expect("parses")
            .expect("a window");
        assert_eq!(from, utc("2026-11-01T04:00:00Z"));
        assert_eq!(to, utc("2026-12-01T05:00:00Z"));
    }

    #[test]
    fn from_alone_runs_to_the_lookahead() {
        let (from, to) = bounds(Some("2026-10-01"), None, NY, 30, utc(NOW))
            .expect("parses")
            .expect("a window");
        assert_eq!(from, utc("2026-10-01T04:00:00Z"));
        assert_eq!(to, utc(NOW) + Duration::days(30));
    }

    #[test]
    fn to_alone_starts_now() {
        let (from, to) = bounds(None, Some("2026-10-31"), NY, 30, utc(NOW))
            .expect("parses")
            .expect("a window");
        assert_eq!(from, utc(NOW));
        assert_eq!(to, utc("2026-11-01T04:00:00Z"));
    }

    #[test]
    fn one_day_is_a_whole_day() {
        let (from, to) = bounds(Some("2026-10-06"), Some("2026-10-06"), NY, 30, utc(NOW))
            .expect("parses")
            .expect("a window");
        assert_eq!(to - from, Duration::days(1));
    }

    #[test]
    fn the_zone_is_the_spaces_own() {
        let (from, _) = bounds(
            Some("2026-10-01"),
            Some("2026-10-31"),
            chrono_tz::UTC,
            30,
            utc(NOW),
        )
        .expect("parses")
        .expect("a window");
        // Anti-vacuity for the assertions above: with a different zone the
        // same date is a different instant, so they are reading the argument.
        assert_eq!(from, utc("2026-10-01T00:00:00Z"));
    }

    #[test]
    fn a_date_that_is_not_a_date_is_refused() {
        for bad in ["nonsense", "2026-13-01", "10/01/2026", "2026-10", ""] {
            assert_eq!(
                bounds(Some(bad), None, NY, 30, utc(NOW)),
                Err(WindowError::Unparseable),
                "{bad:?}"
            );
            assert_eq!(
                bounds(None, Some(bad), NY, 30, utc(NOW)),
                Err(WindowError::Unparseable),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_backwards_window_is_refused_rather_than_answered_empty() {
        // Answering an empty list would read as "nothing is on in October".
        assert_eq!(
            bounds(Some("2026-10-31"), Some("2026-10-01"), NY, 30, utc(NOW)),
            Err(WindowError::Inverted)
        );
    }

    #[test]
    fn a_to_in_the_past_with_no_from_is_refused() {
        // `from` defaults to now, so a `to` before today inverts the window.
        assert_eq!(
            bounds(None, Some("2020-01-01"), NY, 30, utc(NOW)),
            Err(WindowError::Inverted)
        );
    }

    #[test]
    fn every_error_says_something_a_caller_can_act_on() {
        for error in [WindowError::Unparseable, WindowError::Inverted] {
            assert!(!error.message().is_empty());
            // And nothing internal: these reach an unauthenticated caller.
            assert!(!error.message().contains("Error"), "{:?}", error);
        }
    }
}
