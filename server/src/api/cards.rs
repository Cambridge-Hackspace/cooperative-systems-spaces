//! Admin management of member access cards (#33).
//!
//! Cards are first-class and lifecycle-aware: a member may hold several, each
//! `active` / `disabled` / `released`. Only `active` cards open tools/doors;
//! disabling or releasing revokes a single credential without touching the
//! member. Every action here is audited.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use super::errors::ApiError;
use super::responses::ApiResponse;
use crate::auth::AdminUser;
use crate::models::{AuditEventType, NewAuditLog, UserCard};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct IssueCardRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct DisableCardRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/user/{user_id}", get(list_user_cards).post(issue_card))
        .route("/{card_id}/disable", post(disable_card))
        .route("/{card_id}/release", post(release_card))
}

/// List every card belonging to a member (any status), newest first.
async fn list_user_cards(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<UserCard>>>, ApiError> {
    Ok(Json(ApiResponse::success(
        state.db.list_user_cards(user_id)?,
    )))
}

/// Issue a new active card to a member. A code already held by a live
/// (active/disabled) card is rejected with 409 via the unique index.
async fn issue_card(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(user_id): Path<Uuid>,
    Json(req): Json<IssueCardRequest>,
) -> Result<Json<ApiResponse<UserCard>>, ApiError> {
    let code = req.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest(
            "Card code must not be empty".to_string(),
        ));
    }
    let card = state.db.create_card(user_id, code)?;
    audit(&state, AuditEventType::CardIssued, &card, admin.0.id, None);
    Ok(Json(ApiResponse::success(card)))
}

/// Disable a card: revoke access, keep it bound to the member (lost / suspected
/// compromised / member on hold).
async fn disable_card(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(card_id): Path<Uuid>,
    Json(req): Json<DisableCardRequest>,
) -> Result<Json<ApiResponse<UserCard>>, ApiError> {
    let card = state.db.disable_card(card_id, req.reason.as_deref())?;
    audit(
        &state,
        AuditEventType::CardDisabled,
        &card,
        admin.0.id,
        req.reason.clone(),
    );
    Ok(Json(ApiResponse::success(card)))
}

/// Release a card back to the pool: revoke access; the code may be reissued to
/// a different member later.
async fn release_card(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(card_id): Path<Uuid>,
) -> Result<Json<ApiResponse<UserCard>>, ApiError> {
    let card = state.db.release_card(card_id)?;
    audit(
        &state,
        AuditEventType::CardReleased,
        &card,
        admin.0.id,
        None,
    );
    Ok(Json(ApiResponse::success(card)))
}

/// Best-effort audit write for a card lifecycle action.
fn audit(
    state: &AppState,
    event: AuditEventType,
    card: &UserCard,
    actor: Uuid,
    reason: Option<String>,
) {
    let audit_log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: Some(card.user_id),
        actor_id: Some(actor),
        event_data: serde_json::json!({
            "card_id": card.id,
            "card_code": card.code,
            "card_status": card.status,
            "reason": reason,
        }),
        ip_address: None,
        user_agent: None,
    };
    let _ = state.db.create_audit_log(&audit_log);
}
