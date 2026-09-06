//! Groups.io mailing-list HTTP surface.
//!
//! Member-facing self-service for the mailing list: read and set your own
//! subscription. Every handler refuses when the module is disabled, mirroring
//! the doors `require_enabled` guard, so the routes exist unconditionally (the
//! structural checks expect a stable router shape) but do nothing when off.
//!
//! The opt-out state lives in `users.mailing_list_opt_out_at`: `None` is
//! subscribed-by-default (effective once the account is active and the email is
//! verified). Setting it emits a `MailingListSubscribe` / `MailingListUnsubscribe`
//! audit event, which the sync consumer turns into a Groups.io add/remove.

use axum::{
    extract::State,
    response::Json,
    routing::{get, put},
    Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::AuthUser,
    models::AuditEventType,
    AppState,
};

/// Refuse when the Groups.io module is switched off in server config.
fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().groupsio.enabled {
        return Err(ApiError::Forbidden(
            "Groups.io integration is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

/// A member's own view of their mailing-list subscription.
#[derive(Debug, Serialize)]
pub struct SubscriptionStatus {
    /// Whether the member intends to be on the list (opt-out is unset).
    pub subscribed: bool,
    /// Whether the address is verified. Subscription only takes effect once it
    /// is, so the UI can explain a not-yet-effective subscription.
    pub email_verified: bool,
}

/// Request body for changing your own subscription.
#[derive(Debug, Deserialize)]
pub struct SetSubscriptionRequest {
    pub subscribed: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/subscription", get(get_subscription))
        .route("/subscription", put(set_subscription))
}

/// GET /api/groupsio/subscription -- the authenticated member's own state.
async fn get_subscription(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<SubscriptionStatus>>, ApiError> {
    require_enabled(&state)?;
    let u = &user.0;
    Ok(Json(ApiResponse::success(SubscriptionStatus {
        subscribed: u.mailing_list_opt_out_at.is_none(),
        email_verified: u.email_verified_at.is_some(),
    })))
}

/// PUT /api/groupsio/subscription -- the member sets their own subscription.
///
/// Writes only when the intent actually changes, so a no-op save does not churn
/// an audit event (and therefore does not trigger a redundant Groups.io call).
async fn set_subscription(
    user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<SetSubscriptionRequest>,
) -> Result<Json<ApiResponse<SubscriptionStatus>>, ApiError> {
    require_enabled(&state)?;
    let u = &user.0;
    let currently_subscribed = u.mailing_list_opt_out_at.is_none();

    if payload.subscribed != currently_subscribed {
        let opt_out_at = if payload.subscribed {
            None
        } else {
            Some(chrono::Utc::now())
        };
        state
            .db
            .set_mailing_list_opt_out(u.id, opt_out_at)
            .map_err(ApiError::from)?;

        let event = if payload.subscribed {
            AuditEventType::MailingListSubscribe
        } else {
            AuditEventType::MailingListUnsubscribe
        };
        // Actor is the subject: a member acting on their own subscription.
        if let Err(e) = state
            .audit_logger
            .log_event(
                event,
                Some(u.id),
                Some(u.id),
                serde_json::json!({
                    "email": u.email,
                    "subscribed": payload.subscribed,
                }),
                None,
                None,
            )
            .await
        {
            tracing::warn!("Failed to log mailing-list subscription change: {}", e);
        }
    }

    Ok(Json(ApiResponse::success(SubscriptionStatus {
        subscribed: payload.subscribed,
        email_verified: u.email_verified_at.is_some(),
    })))
}
