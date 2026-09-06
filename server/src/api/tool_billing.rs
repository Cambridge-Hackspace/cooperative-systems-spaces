//! Tool-billing HTTP surface (Phase 2).
//!
//! A member reads their spendable balance (ledger balance minus open holds), and
//! admins read the module status and a member's tool-use sessions. There is no
//! refund endpoint here: a correction is an `adjustment` ledger entry via the
//! existing `POST /api/admin/membership/payments`.
//!
//! Every handler refuses when the module is disabled (mirroring the doors
//! `require_enabled` guard), so the router shape stays stable.

use axum::{
    extract::{Path, State},
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::{AdminUser, AuthUser},
    models::ToolUsageSession,
    AppState,
};

fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().tool_billing.enabled {
        return Err(ApiError::Forbidden(
            "Tool billing is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(get_my_tool_billing))
}

/// Admin-only routes, nested under `/api/admin/tool-billing`.
pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(admin_status))
        .route("/users/{id}/sessions", get(admin_user_sessions))
}

/// A member's spendable balance for tool use.
#[derive(Debug, Serialize)]
pub struct ToolBillingView {
    /// Total ledger balance.
    pub balance: String,
    /// Reserved by still-open tool sessions (prepaid holds).
    pub held: String,
    /// balance - held: what a new tool activation can draw on.
    pub available: String,
    pub currency: String,
}

/// GET /api/tool-billing -- the authenticated member's spendable balance.
async fn get_my_tool_billing(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ToolBillingView>>, ApiError> {
    require_enabled(&state)?;
    let balance = state.db.user_balance(user.0.id).map_err(ApiError::from)?;
    let held = state
        .db
        .sum_open_tool_holds(user.0.id)
        .map_err(ApiError::from)?;
    let available = balance.clone() - held.clone();
    let cfg = state.config_manager.get_config();
    Ok(Json(ApiResponse::success(ToolBillingView {
        balance: balance.with_scale(2).to_string(),
        held: held.with_scale(2).to_string(),
        available: available.with_scale(2).to_string(),
        currency: cfg.tool_billing.currency,
    })))
}

/// The admin's view of the module.
#[derive(Debug, Serialize)]
pub struct ToolBillingStatus {
    pub enabled: bool,
    pub billing_mode: crate::config::BillingMode,
    pub actuation_mode: crate::config::ActuationMode,
    pub require_membership: bool,
    pub currency: String,
}

/// GET /api/admin/tool-billing/status
async fn admin_status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ToolBillingStatus>>, ApiError> {
    require_enabled(&state)?;
    let c = state.config_manager.get_config();
    Ok(Json(ApiResponse::success(ToolBillingStatus {
        enabled: true,
        billing_mode: c.tool_billing.billing_mode,
        actuation_mode: c.tool_billing.actuation_mode,
        require_membership: c.tool_billing.require_membership,
        currency: c.tool_billing.currency.clone(),
    })))
}

/// GET /api/admin/tool-billing/users/{id}/sessions -- a member's recent
/// tool-use sessions (holds, charges, status), newest first.
async fn admin_user_sessions(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<ToolUsageSession>>>, ApiError> {
    require_enabled(&state)?;
    let sessions = state
        .db
        .list_tool_sessions_for_user(user_id, 200)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(sessions)))
}
