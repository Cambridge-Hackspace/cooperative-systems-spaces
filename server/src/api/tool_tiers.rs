//! Per-member tool rate tiers (#34): named per-tool rate tiers and per-(member,
//! tool) assignments, admin-managed under `/api/admin/tool-tiers`.
//!
//! A tool's own `usage_*` columns are the default ("Standard") rate; a tier is a
//! named alternative a member can be assigned to (e.g. Discounted, Free). The
//! charge resolves the member's assigned tier, else the tool default -- see
//! [`crate::database::DatabaseManager::resolve_effective_billing`]. Nothing here
//! charges money; it shapes the rate a later tool-use resolves.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::models::{
    AuditEventType, NewAuditLog, NewToolRateTier, NewToolTierAssignment, UpdateToolRateTier,
};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/tools/{tool_id}/tiers", get(list_tiers).post(create_tier))
        .route(
            "/tiers/{tier_id}",
            axum::routing::patch(update_tier).delete(delete_tier),
        )
        .route("/tools/{tool_id}/assignments", get(list_assignments))
        .route(
            "/users/{user_id}/tools/{tool_id}",
            put(assign_tier).delete(clear_tier),
        )
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateTierRequest {
    pub name: String,
    #[serde(default)]
    pub flat_fee: Option<BigDecimal>,
    #[serde(default)]
    pub rate_per_min: Option<BigDecimal>,
    #[serde(default)]
    pub max_session_minutes: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct AssignTierRequest {
    pub tier_id: Uuid,
}

#[derive(Debug, Serialize)]
struct AssignmentView {
    user_id: Uuid,
    tool_id: Uuid,
    tier_id: Uuid,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().tool_billing.enabled {
        return Err(ApiError::Forbidden(
            "Tool billing is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

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
        tracing::error!("Failed to write tier audit log {}: {}", event.as_str(), e);
    }
}

fn reject_negative(flat: &Option<BigDecimal>, rate: &Option<BigDecimal>) -> Result<(), ApiError> {
    let zero = BigDecimal::from(0);
    if flat.as_ref().is_some_and(|v| v < &zero) || rate.as_ref().is_some_and(|v| v < &zero) {
        return Err(ApiError::BadRequest(
            "flat_fee and rate_per_min cannot be negative".to_string(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tier CRUD
// ---------------------------------------------------------------------------

async fn list_tiers(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(tool_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(ApiResponse::success(
        state.db.list_tiers_for_tool(tool_id)?,
    )))
}

async fn create_tier(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(tool_id): Path<Uuid>,
    Json(req): Json<CreateTierRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    reject_negative(&req.flat_fee, &req.rate_per_min)?;
    // The tool must exist (else a raw FK 409); 400 is the honest answer.
    state
        .db
        .get_tool_by_id(tool_id)?
        .ok_or_else(|| ApiError::BadRequest("tool does not exist".to_string()))?;

    let created = state
        .db
        .create_tier(&NewToolRateTier {
            tool_id,
            name: req.name.trim().to_string(),
            flat_fee: req.flat_fee,
            rate_per_min: req.rate_per_min,
            max_session_minutes: req.max_session_minutes,
        })
        .map_err(ApiError::from)?;

    audit(
        &state,
        AuditEventType::ToolRateTierCreated,
        Some(admin.0.id),
        serde_json::json!({ "tier_id": created.id, "tool_id": tool_id, "name": created.name }),
    );
    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn update_tier(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(tier_id): Path<Uuid>,
    Json(mut req): Json<UpdateToolRateTier>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    req.updated_at = Some(chrono::Utc::now());
    let after = state
        .db
        .update_tier(tier_id, &req)
        .map_err(ApiError::from)?;
    audit(
        &state,
        AuditEventType::ToolRateTierUpdated,
        Some(admin.0.id),
        serde_json::json!({ "tier_id": tier_id }),
    );
    Ok(Json(ApiResponse::success(after)))
}

async fn delete_tier(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(tier_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let deleted = state.db.delete_tier(tier_id).map_err(ApiError::from)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Tier not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::ToolRateTierDeleted,
        Some(admin.0.id),
        serde_json::json!({ "tier_id": tier_id }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Tier deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Assignments
// ---------------------------------------------------------------------------

async fn list_assignments(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(tool_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(ApiResponse::success(
        state.db.list_tier_assignments_for_tool(tool_id)?,
    )))
}

async fn assign_tier(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((user_id, tool_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AssignTierRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    // The tier must exist and belong to this tool -- otherwise a member could be
    // put on another tool's rate.
    let tier = state
        .db
        .get_tier(req.tier_id)
        .map_err(|_| ApiError::BadRequest("tier does not exist".to_string()))?;
    if tier.tool_id != tool_id {
        return Err(ApiError::BadRequest(
            "tier does not belong to this tool".to_string(),
        ));
    }

    let assignment = state
        .db
        .upsert_tier_assignment(&NewToolTierAssignment {
            user_id,
            tool_id,
            tier_id: req.tier_id,
            assigned_by: Some(admin.0.id),
        })
        .map_err(ApiError::from)?;

    audit(
        &state,
        AuditEventType::ToolTierAssigned,
        Some(admin.0.id),
        serde_json::json!({ "user_id": user_id, "tool_id": tool_id, "tier_id": req.tier_id }),
    );
    Ok(Json(ApiResponse::success(AssignmentView {
        user_id: assignment.user_id,
        tool_id: assignment.tool_id,
        tier_id: assignment.tier_id,
    })))
}

async fn clear_tier(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((user_id, tool_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let cleared = state
        .db
        .clear_tier_assignment(user_id, tool_id)
        .map_err(ApiError::from)?;
    if cleared == 0 {
        return Err(ApiError::NotFound(
            "No tier assignment for this member and tool".to_string(),
        ));
    }
    audit(
        &state,
        AuditEventType::ToolTierUnassigned,
        Some(admin.0.id),
        serde_json::json!({ "user_id": user_id, "tool_id": tool_id }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Tier assignment cleared".to_string()),
        error: None,
    }))
}
