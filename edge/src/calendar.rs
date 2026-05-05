use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Shape returned by the server's GET /api/calendar/events.
/// Only the fields the edge needs are deserialized.
#[derive(Debug, Deserialize)]
pub struct ServerCalendarEvent {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub calendar_color: String,
    pub all_day: bool,
}

/// Shape published to the local MQTT broker.
/// Must match the kiosk's `CalEvent` type exactly.
#[derive(Debug, Serialize)]
pub struct CalEvent {
    pub summary: String,
    pub start: DateTime<Utc>,
    pub all_day: bool,
    pub is_ongoing: bool,
    pub color_hex: String,
}

pub fn to_cal_event(e: ServerCalendarEvent) -> CalEvent {
    let now = Utc::now();
    let end = e.end.unwrap_or_else(|| e.start + Duration::hours(1));
    let is_ongoing = e.start <= now && end > now;
    CalEvent {
        summary: e.title,
        start: e.start,
        all_day: e.all_day,
        is_ongoing,
        color_hex: e.calendar_color,
    }
}
