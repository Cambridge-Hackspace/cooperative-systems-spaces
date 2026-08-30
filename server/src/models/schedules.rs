use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::schedules;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = schedules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Schedule {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// JSONB array of `{day, start, end}` entries; validated at the API layer.
    /// See `crate::schedules::parse_intervals`.
    pub intervals: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When true, the schedule is surfaced on the public "Hours today" card.
    pub is_public: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = schedules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewSchedule {
    pub name: String,
    pub description: Option<String>,
    pub intervals: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub is_public: bool,
}

#[derive(Debug, Clone, AsChangeset, Default)]
#[diesel(table_name = schedules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateSchedule {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub intervals: Option<serde_json::Value>,
    pub updated_at: Option<DateTime<Utc>>,
    pub is_public: Option<bool>,
}
