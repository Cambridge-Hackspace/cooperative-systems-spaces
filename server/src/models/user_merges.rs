//! The record of one user merged into another (#118). See the migration.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::user_merges;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = user_merges)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserMerge {
    pub id: Uuid,
    pub survivor_id: Option<Uuid>,
    /// No foreign key: the row it names was deleted by the merge.
    pub absorbed_id: Uuid,
    pub absorbed_username: String,
    pub absorbed_email: String,
    /// The absorbed `users` row as JSON, minus `password_hash`.
    pub absorbed_snapshot: serde_json::Value,
    /// Rows re-pointed, per table and column.
    pub moved: serde_json::Value,
    /// The warnings the administrator acknowledged before committing.
    pub warnings: serde_json::Value,
    pub actor_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_merges)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserMerge {
    pub survivor_id: Option<Uuid>,
    pub absorbed_id: Uuid,
    pub absorbed_username: String,
    pub absorbed_email: String,
    pub absorbed_snapshot: serde_json::Value,
    pub moved: serde_json::Value,
    pub warnings: serde_json::Value,
    pub actor_id: Option<Uuid>,
}
