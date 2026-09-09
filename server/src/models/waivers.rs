use crate::schema::training_waivers;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A record that a member's training requirement for a tool is waived.
///
/// The `reason` is mandatory and is the durable provenance — unlike a
/// training-progress note it is the record itself, not a field that cascades
/// away with a training step. Migrated ToolPass grants are stored as waivers
/// (`reason = "migrated from ToolPass"`); the same primitive covers staff
/// discretion, external certification, and grandfathering.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = training_waivers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TrainingWaiver {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tool_id: Uuid,
    pub reason: String,
    /// The admin who granted the waiver; `None` if that user was later deleted
    /// (ON DELETE SET NULL) or unknown (e.g. a migration with no mapped actor).
    pub waived_by: Option<Uuid>,
    /// When the waiver takes/took effect — a real grant date where the source
    /// provides one, else a migration fallback.
    pub waived_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert shape for a waiver. `waived_at` defaults to now at the DB level when
/// omitted; a migration supplies a historical value.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = training_waivers)]
pub struct NewTrainingWaiver {
    pub user_id: Uuid,
    pub tool_id: Uuid,
    pub reason: String,
    pub waived_by: Option<Uuid>,
    pub waived_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}
