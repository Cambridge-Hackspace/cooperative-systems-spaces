//! Weekly-window schedule helpers — validation and "is now within the
//! window?" evaluation in the configured site time zone.

use chrono::{Datelike, NaiveTime, Timelike, Weekday};
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
}
