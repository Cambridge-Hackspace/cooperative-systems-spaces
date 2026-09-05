//! Weekly-window schedule helpers — validation and "is now within the
//! window?" evaluation in the configured site time zone.

use chrono::{Datelike, NaiveTime, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// Day-of-week token used in the JSON shape. Lowercase 3-letter so it
/// reads well in the DB and is unambiguous (`mon` != `mo` for "Monaco").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DayOfWeek {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl DayOfWeek {
    pub fn from_chrono(w: Weekday) -> Self {
        match w {
            Weekday::Mon => Self::Mon,
            Weekday::Tue => Self::Tue,
            Weekday::Wed => Self::Wed,
            Weekday::Thu => Self::Thu,
            Weekday::Fri => Self::Fri,
            Weekday::Sat => Self::Sat,
            Weekday::Sun => Self::Sun,
        }
    }
}

/// One weekly interval. `start` and `end` are minutes-of-day in 24-hour
/// time; the JSON shape uses `"HH:MM"` strings (deserialized via a
/// custom `time` helper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInterval {
    pub day: DayOfWeek,
    #[serde(with = "hhmm")]
    pub start: NaiveTime,
    #[serde(with = "hhmm")]
    pub end: NaiveTime,
}

mod hhmm {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &NaiveTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{:02}:{:02}", t.hour(), t.minute()))
    }
    use chrono::Timelike;
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveTime, D::Error> {
        let raw = String::deserialize(d)?;
        NaiveTime::parse_from_str(&raw, "%H:%M")
            .map_err(|e| serde::de::Error::custom(format!("invalid HH:MM '{raw}': {e}")))
    }
}

/// Parse a `serde_json::Value` (as stored in `schedules.intervals`) into the
/// typed shape. Returns a vector of intervals; empty is fine ("never").
pub fn parse_intervals(value: &serde_json::Value) -> Result<Vec<ScheduleInterval>, String> {
    serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid schedule intervals JSON: {e}"))
}

/// Validate a collection of intervals at API boundary time.
pub fn validate(intervals: &[ScheduleInterval]) -> Result<(), String> {
    for (i, iv) in intervals.iter().enumerate() {
        if iv.end <= iv.start {
            return Err(format!(
                "interval #{i} end ({}) must be strictly after start ({})",
                iv.end, iv.start
            ));
        }
    }
    Ok(())
}

/// True iff *now*, expressed in `tz`, falls within any interval.
pub fn matches_now(intervals: &[ScheduleInterval], tz: Tz) -> bool {
    matches_at(intervals, tz, chrono::Utc::now())
}

/// True iff `ts` (UTC), once shifted into `tz`, falls within any interval.
pub fn matches_at(
    intervals: &[ScheduleInterval],
    tz: Tz,
    ts: chrono::DateTime<chrono::Utc>,
) -> bool {
    let local = ts.with_timezone(&tz);
    let dow = DayOfWeek::from_chrono(local.weekday());
    let now = local.time();
    intervals
        .iter()
        .any(|iv| iv.day == dow && iv.start <= now && now < iv.end)
}

/// If `ts` (UTC), shifted into `tz`, falls inside an open window, return the
/// UTC instant that window ends; otherwise `None`.
///
/// "The window" is the maximal *contiguous same-day* open span containing `now`:
/// intervals on the current local day are chained across adjacency and overlap,
/// so `[09:00-12:00, 12:00-17:00]` yields `17:00` while a genuine gap in
/// `[09:00-12:00, 13:00-17:00]` yields `12:00` (the door locks over the gap and
/// the ticker reopens it at 13:00). This is the value published to the edge as
/// `hold_unlock_until` for an Open Access door.
///
/// This weekly-window model cannot express a window crossing midnight (an
/// interval's `end` must be a same-day `HH:MM` strictly after its `start`), so a
/// span is always bounded by end-of-day; contiguity is only evaluated within the
/// local day. That matches how `matches_at` reads the same data.
pub fn active_until(
    intervals: &[ScheduleInterval],
    tz: Tz,
    ts: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{Datelike, LocalResult, TimeZone};

    let local = ts.with_timezone(&tz);
    let dow = DayOfWeek::from_chrono(local.weekday());
    let now_t = local.time();
    let date = local.date_naive();

    // Same-day intervals, earliest start first, so chaining is a single pass.
    let mut day: Vec<&ScheduleInterval> = intervals.iter().filter(|iv| iv.day == dow).collect();
    day.sort_by_key(|iv| iv.start);

    // The interval currently containing `now` (end exclusive, as in `matches_at`).
    let mut end = day
        .iter()
        .find(|iv| iv.start <= now_t && now_t < iv.end)?
        .end;

    // Extend forward across any interval that touches or overlaps the running
    // end. `end` strictly increases each step, so this terminates.
    while let Some(iv) = day.iter().find(|iv| iv.start <= end && iv.end > end) {
        end = iv.end;
    }

    // Re-anchor the local end-of-window onto the local date and convert to UTC.
    let naive_end = date.and_time(end);
    match tz.from_local_datetime(&naive_end) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => {
            Some(dt.with_timezone(&chrono::Utc))
        }
        // DST spring-forward can make the exact end instant nonexistent. Prefer
        // holding open a minute longer (fail toward the safe direction for a
        // *closing* boundary) over locking early.
        LocalResult::None => tz
            .from_local_datetime(&(naive_end - chrono::Duration::minutes(1)))
            .single()
            .map(|dt| dt.with_timezone(&chrono::Utc)),
    }
}

/// Resolve a `Tz` from an IANA name. Falls back to UTC with a warning.
pub fn resolve_tz(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or_else(|_| {
        tracing::warn!("Unknown site.timezone '{name}'; falling back to UTC");
        chrono_tz::UTC
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Needed for `.hour()` below. It used to come in through the top-level
    // import, which was unused in a non-test build and therefore warned --
    // removing it there and adding it here is what makes both builds clean.
    use chrono::Timelike;
    use chrono::{TimeZone, Utc};

    fn iv(day: DayOfWeek, s: &str, e: &str) -> ScheduleInterval {
        ScheduleInterval {
            day,
            start: NaiveTime::parse_from_str(s, "%H:%M").unwrap(),
            end: NaiveTime::parse_from_str(e, "%H:%M").unwrap(),
        }
    }

    #[test]
    fn json_round_trip() {
        let raw = serde_json::json!([
            { "day": "mon", "start": "09:00", "end": "17:00" },
            { "day": "fri", "start": "08:30", "end": "12:00" }
        ]);
        let parsed = parse_intervals(&raw).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].day, DayOfWeek::Mon);
        assert_eq!(parsed[1].end.hour(), 12);
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn validate_rejects_inverted() {
        let bad = vec![iv(DayOfWeek::Mon, "17:00", "09:00")];
        assert!(validate(&bad).is_err());
    }

    #[test]
    fn matches_business_hours_in_chicago() {
        // 2026-05-29 is a Friday. 14:00 CDT is 19:00 UTC.
        let intervals = vec![iv(DayOfWeek::Fri, "09:00", "17:00")];
        let tz: Tz = "America/Chicago".parse().unwrap();
        let inside = Utc.with_ymd_and_hms(2026, 5, 29, 19, 0, 0).unwrap();
        let outside = Utc.with_ymd_and_hms(2026, 5, 29, 23, 0, 0).unwrap();
        assert!(matches_at(&intervals, tz, inside));
        assert!(!matches_at(&intervals, tz, outside));
    }

    #[test]
    fn matches_handles_weekday_boundaries() {
        let intervals = vec![iv(DayOfWeek::Mon, "09:00", "10:00")];
        let tz = chrono_tz::UTC;
        // Monday 09:30 UTC — inside.
        let inside = Utc.with_ymd_and_hms(2026, 6, 1, 9, 30, 0).unwrap();
        // Tuesday 09:30 — outside (wrong day).
        let wrong_day = Utc.with_ymd_and_hms(2026, 6, 2, 9, 30, 0).unwrap();
        // Monday 10:00 — outside (end is exclusive).
        let edge = Utc.with_ymd_and_hms(2026, 6, 1, 10, 0, 0).unwrap();
        assert!(matches_at(&intervals, tz, inside));
        assert!(!matches_at(&intervals, tz, wrong_day));
        assert!(!matches_at(&intervals, tz, edge));
    }

    #[test]
    fn active_until_returns_window_end_in_utc() {
        // 2026-05-29 is a Friday; CDT = UTC-5. 14:00 CDT = 19:00 UTC, and the
        // window ends 17:00 CDT = 22:00 UTC.
        let intervals = vec![iv(DayOfWeek::Fri, "09:00", "17:00")];
        let tz: Tz = "America/Chicago".parse().unwrap();
        let inside = Utc.with_ymd_and_hms(2026, 5, 29, 19, 0, 0).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 5, 29, 22, 0, 0).unwrap();
        assert_eq!(active_until(&intervals, tz, inside), Some(expected));
    }

    #[test]
    fn active_until_is_none_outside_and_at_exclusive_end() {
        let intervals = vec![iv(DayOfWeek::Mon, "09:00", "10:00")];
        let tz = chrono_tz::UTC;
        // Before the window.
        let before = Utc.with_ymd_and_hms(2026, 6, 1, 8, 30, 0).unwrap();
        // After the window.
        let after = Utc.with_ymd_and_hms(2026, 6, 1, 11, 0, 0).unwrap();
        // Exactly at the exclusive end — not open.
        let at_end = Utc.with_ymd_and_hms(2026, 6, 1, 10, 0, 0).unwrap();
        // Right day-time but wrong weekday.
        let wrong_day = Utc.with_ymd_and_hms(2026, 6, 2, 9, 30, 0).unwrap();
        assert_eq!(active_until(&intervals, tz, before), None);
        assert_eq!(active_until(&intervals, tz, after), None);
        assert_eq!(active_until(&intervals, tz, at_end), None);
        assert_eq!(active_until(&intervals, tz, wrong_day), None);
    }

    #[test]
    fn active_until_merges_adjacent_and_overlapping_spans() {
        let tz = chrono_tz::UTC;
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 10, 0, 0).unwrap(); // Mon 10:00
        let end_1700 = Utc.with_ymd_and_hms(2026, 6, 1, 17, 0, 0).unwrap();

        // Adjacent: 09-12 then 12-17 chain to 17:00.
        let adjacent = vec![
            iv(DayOfWeek::Mon, "09:00", "12:00"),
            iv(DayOfWeek::Mon, "12:00", "17:00"),
        ];
        assert_eq!(active_until(&adjacent, tz, now), Some(end_1700));

        // Overlapping: 09-13 and 12-17 chain to 17:00.
        let overlapping = vec![
            iv(DayOfWeek::Mon, "09:00", "13:00"),
            iv(DayOfWeek::Mon, "12:00", "17:00"),
        ];
        assert_eq!(active_until(&overlapping, tz, now), Some(end_1700));
    }

    #[test]
    fn active_until_stops_at_a_real_gap() {
        // A lunch gap must NOT be bridged: at 10:00 the span ends at 12:00, and
        // the 12:00-13:00 gap reads as closed (the door locks, then reopens).
        let tz = chrono_tz::UTC;
        let intervals = vec![
            iv(DayOfWeek::Mon, "09:00", "12:00"),
            iv(DayOfWeek::Mon, "13:00", "17:00"),
        ];
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 10, 0, 0).unwrap();
        let gap = Utc.with_ymd_and_hms(2026, 6, 1, 12, 30, 0).unwrap();
        let end_1200 = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        assert_eq!(active_until(&intervals, tz, now), Some(end_1200));
        assert_eq!(active_until(&intervals, tz, gap), None);
    }
}
