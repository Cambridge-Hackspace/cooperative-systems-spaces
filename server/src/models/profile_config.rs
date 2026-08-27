use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::profile_config_versions;

/// A single, immutable snapshot of the admin-editable user-profile field
/// schema. Rows are append-only: the current schema is the row with the
/// highest `version`, and a rollback is inserting a new row carrying an
/// older row's `profile_fields`.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, Identifiable)]
#[diesel(table_name = profile_config_versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ProfileConfigVersion {
    pub id: Uuid,
    pub version: i64,
    pub profile_fields: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub profiles_enabled: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = profile_config_versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewProfileConfigVersion {
    pub version: i64,
    pub profile_fields: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub profiles_enabled: bool,
}
