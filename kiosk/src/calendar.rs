use bevy::prelude::*;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

// ── Wire type ─────────────────────────────────────────────────────────────────
//
// Calendar events arrive over MQTT as a JSON array of CalEvent.  The iCal
// fetching and pre-processing is done by a publisher (server/edge/standalone);
// the kiosk is purely a subscriber.
//
// Example payload:
//   [{"summary":"Workshop","start":"2026-03-31T14:00:00Z",
//     "all_day":false,"is_ongoing":false,"color_hex":"#e85d9b"}]

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalEvent {
    pub summary: String,
    /// RFC 3339 UTC timestamp.
    pub start: DateTime<Utc>,
    pub all_day: bool,
    pub is_ongoing: bool,
    /// Hex color string, e.g. "#e85d9b".
    pub color_hex: String,
}

impl CalEvent {
    pub fn start_local(&self) -> DateTime<Local> {
        self.start.with_timezone(&Local)
    }

    pub fn display_color(&self) -> Color {
        parse_hex_color(&self.color_hex)
    }

    pub fn date_str(&self) -> String {
        self.start_local().format("%b %-d").to_string()
    }

    pub fn time_str(&self) -> String {
        if self.all_day {
            "all day".to_string()
        } else {
            self.start_local().format("%H:%M").to_string()
        }
    }
}

pub fn parse_hex_color(s: &str) -> Color {
    let s = s.trim_start_matches('#');
    if s.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
        ) {
            return Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
        }
    }
    Color::WHITE
}

// ── Bevy resource ─────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
pub struct CalendarData {
    pub events: Vec<CalEvent>,
    pub generation: u32,
}

impl CalendarData {
    pub fn update(&mut self, mut events: Vec<CalEvent>) {
        // Ongoing events first, then chronologically ascending.
        events.sort_by(|a, b| b.is_ongoing.cmp(&a.is_ongoing).then(a.start.cmp(&b.start)));
        events.truncate(12);
        self.events = events;
        self.generation = self.generation.wrapping_add(1);
    }
}
