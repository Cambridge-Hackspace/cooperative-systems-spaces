use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::places;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = places)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Place {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub place_type: String,
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When true, this place is outside the normal hierarchy (`Outside`,
    /// `Public Area`, `Parking Lot`, …). The configured type-ordering and
    /// "type must come from `[place].types`" rules are skipped, and the
    /// place must be a root (no `parent_id`).
    pub is_special: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = places)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewPlace {
    pub parent_id: Option<Uuid>,
    pub place_type: String,
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub is_special: bool,
}

#[derive(Debug, Clone, AsChangeset, Default, Deserialize)]
#[diesel(table_name = places)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdatePlace {
    /// Outer `Option` = "field present in request?"; inner `Option` =
    /// "set to NULL?" so callers can promote a place to a root with
    /// `parent_id: Some(None)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_special: Option<bool>,
}
