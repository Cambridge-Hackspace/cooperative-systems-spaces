//! Reusable weekly-window schedules attachable to door access rules.
//!
//! Admin endpoints live at `/api/admin/schedules`; member-facing listing
//! lives at `/api/schedules` so any future surfacing (member directory,
//! "what hours is this room open?") can reuse the same data.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AdminUser, AuthUser};
use crate::models::{AuditEventType, NewAuditLog, NewSchedule, Schedule, UpdateSchedule};
use crate::schedules::{parse_intervals, validate, ScheduleInterval};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// Routers
// ---------------------------------------------------------------------------

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_schedules_admin).post(create_schedule))
        .route(
            "/{id}",
            get(get_schedule_admin)
                .patch(update_schedule)
                .delete(delete_schedule),
        )
}

pub fn member_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_schedules_member))
        .route("/{id}", get(get_schedule_member))
}

/// `GET /api/public/schedules` — no auth, returns only `is_public` schedules.
/// Used by the home page "Hours today" card so unauthenticated visitors can
/// see the space's posted hours.
pub fn public_routes() -> Router<AppState> {
    Router::new().route("/schedules", get(list_schedules_public))
}

async fn list_schedules_public(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let rows = state.db.list_schedules()?;
    let resp: Vec<ScheduleResponse> = rows
        .into_iter()
        .filter(|s| s.is_public)
        .filter_map(|s| ScheduleResponse::try_from_schedule(s).ok())
        .collect();
    Ok(Json(ApiResponse::success(resp)))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Array of `{day, start, end}` entries. Empty = "never".
    pub intervals: Vec<ScheduleInterval>,
    /// Default `false`; flip to surface on the public "Hours today" card.
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateScheduleRequest {
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    pub intervals: Option<Vec<ScheduleInterval>>,
    #[serde(default)]
    pub is_public: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ScheduleResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub intervals: Vec<ScheduleInterval>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub is_public: bool,
}

impl ScheduleResponse {
    fn try_from_schedule(s: Schedule) -> Result<Self, ApiError> {
        let intervals = parse_intervals(&s.intervals).map_err(ApiError::InternalServerError)?;
        Ok(Self {
            id: s.id,
            name: s.name,
            description: s.description,
            intervals,
            created_at: s.created_at,
            updated_at: s.updated_at,
            is_public: s.is_public,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn audit(state: &AppState, event: AuditEventType, actor: Option<Uuid>, data: serde_json::Value) {
    let log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: actor,
        actor_id: actor,
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!(
            "Failed to write schedule audit log {}: {}",
            event.as_str(),
            e
        );
    }
}

fn validate_intervals(intervals: &[ScheduleInterval]) -> Result<(), ApiError> {
    validate(intervals).map_err(ApiError::BadRequest)
}

fn map_conflict(e: crate::database::DatabaseError) -> ApiError {
    match e {
        crate::database::DatabaseError::Diesel(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => ApiError::Conflict("A schedule with that name already exists".to_string()),
        other => ApiError::from(other),
    }
}

// ---------------------------------------------------------------------------
// Admin handlers
// ---------------------------------------------------------------------------

async fn list_schedules_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let rows = state.db.list_schedules()?;
    let resp: Vec<ScheduleResponse> = rows
        .into_iter()
        .filter_map(|s| ScheduleResponse::try_from_schedule(s).ok())
        .collect();
    Ok(Json(ApiResponse::success(resp)))
}

async fn get_schedule_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let s = state.db.get_schedule(id)?;
    Ok(Json(ApiResponse::success(
        ScheduleResponse::try_from_schedule(s)?,
    )))
}

async fn create_schedule(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateScheduleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    validate_intervals(&req.intervals)?;
    let intervals_json = serde_json::to_value(&req.intervals)
        .map_err(|e| ApiError::InternalServerError(format!("serialize intervals: {e}")))?;
    let created = state
        .db
        .create_schedule(&NewSchedule {
            name: req.name.trim().to_string(),
            description: req.description,
            intervals: intervals_json,
            created_by: Some(admin.0.id),
            is_public: req.is_public,
        })
        .map_err(map_conflict)?;
    audit(
        &state,
        AuditEventType::ScheduleCreated,
        Some(admin.0.id),
        serde_json::json!({ "schedule_id": created.id, "name": created.name }),
    );
    // No transitions to compute here — rules don't reference this schedule yet.
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(ScheduleResponse::try_from_schedule(
            created,
        )?)),
    ))
}

async fn update_schedule(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateScheduleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let intervals_json = match req.intervals {
        Some(ivs) => {
            validate_intervals(&ivs)?;
            Some(
                serde_json::to_value(&ivs)
                    .map_err(|e| ApiError::InternalServerError(format!("serialize: {e}")))?,
            )
        }
        None => None,
    };
    let updated = state
        .db
        .update_schedule(
            id,
            &UpdateSchedule {
                name: req.name.map(|s| s.trim().to_string()),
                description: req.description,
                intervals: intervals_json,
                updated_at: Some(Utc::now()),
                is_public: req.is_public,
            },
        )
        .map_err(map_conflict)?;
    audit(
        &state,
        AuditEventType::ScheduleUpdated,
        Some(admin.0.id),
        serde_json::json!({ "schedule_id": id }),
    );
    // Any rule that references this schedule may now allow/deny different
    // cards; re-evaluate all door devices.
    state.door_service.republish_all();
    Ok(Json(ApiResponse::success(
        ScheduleResponse::try_from_schedule(updated)?,
    )))
}

async fn delete_schedule(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let prev = state.db.get_schedule(id)?;
    let deleted = state.db.delete_schedule(id)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Schedule not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::ScheduleDeleted,
        Some(admin.0.id),
        serde_json::json!({ "schedule_id": id, "name": prev.name }),
    );
    // FK is `ON DELETE SET NULL` — rules that pointed here are now ungated.
    state.door_service.republish_all();
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Schedule deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Member-facing handlers (read-only)
// ---------------------------------------------------------------------------

async fn list_schedules_member(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let rows = state.db.list_schedules()?;
    let resp: Vec<ScheduleResponse> = rows
        .into_iter()
        .filter_map(|s| ScheduleResponse::try_from_schedule(s).ok())
        .collect();
    Ok(Json(ApiResponse::success(resp)))
}

async fn get_schedule_member(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let s = state.db.get_schedule(id)?;
    Ok(Json(ApiResponse::success(
        ScheduleResponse::try_from_schedule(s)?,
    )))
}
