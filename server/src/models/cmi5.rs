//! Diesel models for the cmi5 subsystem.
//!
//! These mirror the tables created in
//! `migrations/2026-09-04-130000-0000_add_cmi5_tables`. The pure cmi5/xAPI logic
//! (manifest parsing, statement validation, moveOn) lives in the `cmi5` crate;
//! this module is only the persistence shape the server hangs it on.
//!
//! Enum-like columns are stored as `String` (`move_on`, `launch_method`,
//! `launch_mode`) and converted to/from the `cmi5` crate's typed enums at the
//! edges, matching the migration's decision to keep them as TEXT.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::schema::{
    cmi5_assignable_units, cmi5_blocks, cmi5_courses, cmi5_launch_tokens, cmi5_registrations,
    cmi5_state_documents, cmi5_statements,
};

// ---------------------------------------------------------------------------
// Courses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize, Deserialize)]
#[diesel(table_name = cmi5_courses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5Course {
    pub id: Uuid,
    pub course_iri: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content_path: String,
    pub manifest_xml: String,
    pub imported_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_courses)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5Course {
    /// Set explicitly (not defaulted) so the row id matches the content
    /// directory the package was extracted to, which is named by this id.
    pub id: Uuid,
    pub course_iri: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub content_path: String,
    pub manifest_xml: String,
    pub imported_by: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize, Deserialize)]
#[diesel(table_name = cmi5_blocks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5Block {
    pub id: Uuid,
    pub course_id: Uuid,
    pub parent_block_id: Option<Uuid>,
    pub block_iri: Option<String>,
    pub title: Option<String>,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_blocks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5Block {
    pub course_id: Uuid,
    pub parent_block_id: Option<Uuid>,
    pub block_iri: Option<String>,
    pub title: Option<String>,
    pub position: i32,
}

// ---------------------------------------------------------------------------
// Assignable units
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize, Deserialize)]
#[diesel(table_name = cmi5_assignable_units)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5AssignableUnit {
    pub id: Uuid,
    pub course_id: Uuid,
    pub block_id: Option<Uuid>,
    pub au_iri: String,
    pub title: Option<String>,
    pub launch_url: String,
    pub launch_parameters: Option<String>,
    pub launch_method: Option<String>,
    pub move_on: String,
    pub mastery_score: Option<f64>,
    pub position: i32,
    /// The training step this AU is bound to; `None` until an admin maps it.
    /// A verified pass writes this step's progress, granting tool access.
    pub training_step_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_assignable_units)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5AssignableUnit {
    pub course_id: Uuid,
    pub block_id: Option<Uuid>,
    pub au_iri: String,
    pub title: Option<String>,
    pub launch_url: String,
    pub launch_parameters: Option<String>,
    pub launch_method: Option<String>,
    pub move_on: String,
    pub mastery_score: Option<f64>,
    pub position: i32,
    pub training_step_id: Option<Uuid>,
}

/// Bind (or unbind) an AU to a training step. `treat_none_as_null = true` so
/// clearing the binding writes NULL rather than being skipped.
#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = cmi5_assignable_units, treat_none_as_null = true)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct AssignCmi5AuStep {
    pub training_step_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Registrations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize, Deserialize)]
#[diesel(table_name = cmi5_registrations)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5Registration {
    pub id: Uuid,
    pub user_id: Uuid,
    pub au_id: Uuid,
    pub actor_account_name: String,
    pub launch_mode: String,
    pub satisfied_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub passed_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_registrations)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5Registration {
    pub id: Uuid,
    pub user_id: Uuid,
    pub au_id: Uuid,
    pub actor_account_name: String,
    pub launch_mode: String,
}

/// Record the outcomes observed for a registration. `Default` so a handler sets
/// only the columns a given statement changed; unset `None`s are skipped
/// (`treat_none_as_null` left at its default of false).
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = cmi5_registrations)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateCmi5Registration {
    pub satisfied_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub passed_at: Option<DateTime<Utc>>,
    pub failed_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Launch tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = cmi5_launch_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5LaunchToken {
    pub id: Uuid,
    pub registration_id: Uuid,
    pub fetch_token_hash: String,
    pub session_token_hash: Option<String>,
    pub fetch_consumed_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub session_expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_launch_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5LaunchToken {
    pub registration_id: Uuid,
    pub fetch_token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub session_expires_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Statements (the embedded LRS store)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = cmi5_statements)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5Statement {
    pub id: Uuid,
    pub registration_id: Uuid,
    pub statement_id: Uuid,
    pub verb_iri: String,
    pub stored: DateTime<Utc>,
    pub statement: Value,
    pub voided: bool,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_statements)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5Statement {
    pub registration_id: Uuid,
    pub statement_id: Uuid,
    pub verb_iri: String,
    pub statement: Value,
}

// ---------------------------------------------------------------------------
// State documents (State API)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = cmi5_state_documents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Cmi5StateDocument {
    pub id: Uuid,
    pub registration_id: Uuid,
    pub activity_iri: String,
    pub agent_account_name: String,
    pub state_id: String,
    pub document: Value,
    pub etag: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = cmi5_state_documents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCmi5StateDocument {
    pub registration_id: Uuid,
    pub activity_iri: String,
    pub agent_account_name: String,
    pub state_id: String,
    pub document: Value,
    pub etag: String,
}
