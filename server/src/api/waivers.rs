//! Admin management of training waivers (#36).
//!
//! A waiver records that a member's training requirement for a tool is waived,
//! with a mandatory reason. It is honored by the shared access rule
//! (`user_is_authorized_for_tool`), so granting one opens the tool for that
//! member on both the web self-check and the physical interlock. Every action
//! is audited.

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use super::errors::ApiError;
use super::responses::ApiResponse;
use crate::auth::AdminUser;
use crate::models::{AuditEventType, NewAuditLog, NewTrainingWaiver, TrainingWaiver};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct GrantWaiverRequest {
    /// Mandatory: why this training requirement is waived (e.g. "migrated from
    /// ToolPass", "holds external certification").
    pub reason: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/user/{user_id}", get(list_user_waivers))
        .route("/user/{user_id}/tool/{tool_id}", post(grant_waiver))
        .route("/{waiver_id}", delete(revoke_waiver))
}

/// List every waiver a member holds.
async fn list_user_waivers(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<TrainingWaiver>>>, ApiError> {
    Ok(Json(ApiResponse::success(
        state.db.list_waivers_for_user(user_id)?,
    )))
}

/// Grant (or update, keyed on member+tool) a training waiver. The reason is
/// required.
async fn grant_waiver(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((user_id, tool_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<GrantWaiverRequest>,
) -> Result<Json<ApiResponse<TrainingWaiver>>, ApiError> {
    let reason = req.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest(
            "A waiver reason is required".to_string(),
        ));
    }
    let waiver = state.db.upsert_waiver(&NewTrainingWaiver {
        user_id,
        tool_id,
        reason: reason.to_string(),
        waived_by: Some(admin.0.id),
        waived_at: None,
        expires_at: req.expires_at,
    })?;
    audit(
        &state,
        AuditEventType::TrainingWaiverGranted,
        &waiver,
        admin.0.id,
    );
    Ok(Json(ApiResponse::success(waiver)))
}

/// Revoke a waiver by id.
async fn revoke_waiver(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(waiver_id): Path<Uuid>,
) -> Result<Json<ApiResponse<TrainingWaiver>>, ApiError> {
    let waiver = state.db.revoke_waiver(waiver_id)?;
    audit(
        &state,
        AuditEventType::TrainingWaiverRevoked,
        &waiver,
        admin.0.id,
    );
    Ok(Json(ApiResponse::success(waiver)))
}

/// Best-effort audit write for a waiver lifecycle action.
fn audit(state: &AppState, event: AuditEventType, waiver: &TrainingWaiver, actor: Uuid) {
    let audit_log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: Some(waiver.user_id),
        actor_id: Some(actor),
        event_data: serde_json::json!({
            "waiver_id": waiver.id,
            "tool_id": waiver.tool_id,
            "reason": waiver.reason,
            "expires_at": waiver.expires_at,
        }),
        ip_address: None,
        user_agent: None,
    };
    let _ = state.db.create_audit_log(&audit_log);
}
