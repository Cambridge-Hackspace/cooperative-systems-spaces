//! Membership HTTP surface.
//!
//! Member self-service (read your own membership state) and admin operations
//! (status, manual reconcile, log a cash payment, view/adjust a member's
//! ledger). Every handler refuses when the module is disabled, mirroring the
//! doors `require_enabled` guard, so the routes exist unconditionally (the
//! structural checks expect a stable router shape) but do nothing when off.
//!
//! No card data is handled here: online payment goes through Stripe-hosted
//! Checkout/Portal (see `api::stripe`); this module deals only in ledger amounts
//! and the derived membership state.

use std::str::FromStr;

use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, post},
    Router,
};
use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::{AdminUser, AuthUser},
    membership::{MembershipCycleOutcome, MembershipService},
    models::{
        AuditEventType, LedgerEntryType, MembershipLedgerEntry, MembershipSyncRun, NewAuditLog,
    },
    AppState,
};

/// Refuse when the membership module is switched off in server config.
fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().membership.enabled {
        return Err(ApiError::Forbidden(
            "Membership module is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

/// Enabled implies the service was wired at boot; a `None` here is a server
/// inconsistency the caller cannot fix -- hence 500, not 403.
fn service(state: &AppState) -> Result<std::sync::Arc<MembershipService>, ApiError> {
    state.membership.clone().ok_or_else(|| {
        ApiError::InternalServerError("Membership service is not running".to_string())
    })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(get_membership))
}

/// Admin-only routes, nested under `/api/admin/membership`.
pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(admin_status))
        .route("/reconcile", post(admin_reconcile))
        .route("/payments", post(admin_log_payment))
        .route("/users/{id}/ledger", get(admin_user_ledger))
        .route("/users/{id}/next-due", post(admin_set_next_due))
}

/// A member's own view of their membership.
#[derive(Debug, Serialize)]
pub struct MembershipView {
    /// Whether the member has an active membership clock (enrolled in dues).
    pub enrolled: bool,
    /// Whether their role is currently at or above the member role.
    pub is_member: bool,
    /// Current ledger balance, formatted to two places.
    pub balance: String,
    pub currency: String,
    /// Next dues anniversary, if enrolled.
    pub next_due_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether they have a live Stripe subscription (so the SPA can offer
    /// "manage" via the Billing Portal).
    pub has_subscription: bool,
    /// Display label for the subscription/plan.
    pub plan_name: String,
}

/// GET /api/membership -- the authenticated member's own state.
async fn get_membership(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<MembershipView>>, ApiError> {
    require_enabled(&state)?;
    let cfg = state.config_manager.get_config();
    let u = &user.0;
    let balance = state.db.user_balance(u.id).map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(MembershipView {
        enrolled: u.membership_next_due_at.is_some(),
        is_member: u.role.rank() >= cfg.membership.member_role.rank(),
        balance: balance.with_scale(2).to_string(),
        currency: cfg.membership.currency.clone(),
        next_due_at: u.membership_next_due_at,
        has_subscription: u.stripe_subscription_id.is_some(),
        plan_name: cfg.membership.plan_name.clone(),
    })))
}

/// The admin's view of the module.
#[derive(Debug, Serialize)]
pub struct MembershipAdminStatus {
    pub enabled: bool,
    pub stripe_enabled: bool,
    pub enrolled_count: usize,
    pub recent_runs: Vec<MembershipSyncRun>,
}

/// GET /api/admin/membership/status
async fn admin_status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<MembershipAdminStatus>>, ApiError> {
    require_enabled(&state)?;
    let enrolled_count = state.db.enrolled_users().map_err(ApiError::from)?.len();
    let recent_runs = state
        .db
        .latest_membership_sync_runs(20)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(MembershipAdminStatus {
        enabled: true,
        stripe_enabled: state.config_manager.get_config().stripe.enabled,
        enrolled_count,
        recent_runs,
    })))
}

/// POST /api/admin/membership/reconcile -- run a renewal cycle now.
async fn admin_reconcile(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<MembershipCycleOutcome>>, ApiError> {
    require_enabled(&state)?;
    let svc = service(&state)?;
    let outcome = svc.run_cycle().await;
    Ok(Json(ApiResponse::success(outcome)))
}

/// Request to log an off-Stripe payment (or a correction) as a ledger entry.
#[derive(Debug, Deserialize)]
pub struct LogPaymentRequest {
    pub user_id: Uuid,
    /// Decimal amount, e.g. "25.00". Signed: an adjustment may be negative.
    pub amount: String,
    /// "cash_payment" (default) or "adjustment".
    #[serde(default)]
    pub entry_type: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LogPaymentResponse {
    /// Whether a new ledger entry was posted (false only on a no-op).
    pub posted: bool,
    /// The member's balance after the entry.
    pub balance: String,
}

/// POST /api/admin/membership/payments -- record a cash payment or adjustment.
///
/// Modeled on the `create_link` admin-insert pattern: validate, write, audit
/// with the admin as actor. This is the accountability record for money taken
/// outside Stripe; the amount is stored, card data never is (there is none).
async fn admin_log_payment(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(req): Json<LogPaymentRequest>,
) -> Result<Json<ApiResponse<LogPaymentResponse>>, ApiError> {
    require_enabled(&state)?;
    let svc = service(&state)?;

    let amount = BigDecimal::from_str(req.amount.trim())
        .map_err(|_| ApiError::BadRequest(format!("amount {:?} is not a decimal", req.amount)))?;
    if amount == BigDecimal::from(0) {
        return Err(ApiError::BadRequest("amount must not be zero".to_string()));
    }
    let entry_type = match req.entry_type.as_deref() {
        None | Some("cash_payment") => LedgerEntryType::CashPayment,
        Some("adjustment") => LedgerEntryType::Adjustment,
        Some(other) => {
            return Err(ApiError::BadRequest(format!(
                "entry_type {other:?} must be cash_payment or adjustment"
            )))
        }
    };

    let user = state
        .db
        .find_user_by_id(req.user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    let posted = svc
        .record_credit(
            &user,
            entry_type.clone(),
            amount.clone(),
            None,
            req.description.clone(),
            Some(admin.0.id),
        )
        .map_err(ApiError::from)?;

    let log = NewAuditLog {
        event_type: AuditEventType::MembershipPaymentRecorded
            .as_str()
            .to_string(),
        user_id: Some(user.id),
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "amount": amount.with_scale(2).to_string(),
            "entry_type": entry_type.as_str(),
            "description": req.description,
        }),
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write membership payment audit: {e}");
    }

    let balance = state.db.user_balance(user.id).map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(LogPaymentResponse {
        posted,
        balance: balance.with_scale(2).to_string(),
    })))
}

/// GET /api/admin/membership/users/{id}/ledger -- a member's ledger, newest first.
async fn admin_user_ledger(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<MembershipLedgerEntry>>>, ApiError> {
    require_enabled(&state)?;
    let entries = state
        .db
        .list_user_ledger(user_id, 200)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(entries)))
}

/// Request to set (or clear) a member's next-due anniversary.
#[derive(Debug, Deserialize)]
pub struct SetNextDueRequest {
    /// `null` un-enrolls the member (ends their membership clock).
    pub next_due_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// POST /api/admin/membership/users/{id}/next-due -- correct a renewal date.
///
/// A real admin capability (fix a member's renewal anchor, or un-enroll them)
/// that also lets the e2e battery drive a deterministic past-due state.
async fn admin_set_next_due(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<SetNextDueRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    require_enabled(&state)?;
    state
        .db
        .set_membership_next_due(user_id, req.next_due_at)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(())))
}
