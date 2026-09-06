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
    body::Bytes,
    extract::State,
    http::HeaderMap,
    response::Json,
    routing::{get, post, put},
    Router,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::AuthUser,
    models::{AuditEventType, NewAuditLog},
    AppState,
};

type HmacSha256 = Hmac<Sha256>;

/// Header carrying the HMAC-SHA256 of the request body, `sha256=<hex>`.
///
/// The exact header name and signing scheme Groups.io uses are settled against
/// a live account; this is the assumed shape and is the single place to correct
/// it. The reconciliation poll catches every opt-out regardless, so a mismatch
/// here degrades latency, not correctness.
const SIGNATURE_HEADER: &str = "x-groupsio-signature";

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
        .route("/webhook", post(receive_webhook))
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

/// Acknowledgement for an inbound webhook: whether it moved anything.
#[derive(Debug, Serialize)]
pub struct WebhookAck {
    pub handled: bool,
}

/// POST /api/groupsio/webhook -- inbound membership notification from Groups.io.
///
/// The low-latency half of opt-out obedience: when Groups.io reports that a
/// member left or unsubscribed (including from an email link), record a local
/// opt-out so the platform never re-adds them. Public but secret-verified, and
/// refused when the module is off or no webhook secret is set. The
/// reconciliation poll is the backbone and catches the same events on its own,
/// so this endpoint only reduces latency -- and a case-mismatched address that
/// misses the exact lookup here is still caught, normalized, by the poll.
async fn receive_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ApiResponse<WebhookAck>>, ApiError> {
    require_enabled(&state)?;

    let secret = state.config_manager.get_config().groupsio.webhook_secret;
    if secret.trim().is_empty() {
        return Err(ApiError::Forbidden(
            "Groups.io inbound webhook is not configured (no webhook_secret)".to_string(),
        ));
    }

    let provided = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_signature(&secret, &body, provided) {
        return Err(ApiError::Unauthorized(
            "Invalid Groups.io webhook signature".to_string(),
        ));
    }

    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("Webhook body is not JSON".to_string()))?;

    let Some(email) = parse_membership_removal(&parsed) else {
        // A membership event we do not act on (a join, a message, etc.).
        return Ok(Json(ApiResponse::success(WebhookAck { handled: false })));
    };

    // Only record an opt-out for an address we actually know.
    match state.db.find_user_by_email(&email) {
        Ok(Some(user)) => {
            state
                .db
                .set_mailing_list_opt_out(user.id, Some(chrono::Utc::now()))
                .map_err(ApiError::from)?;
            let log = NewAuditLog {
                event_type: AuditEventType::MailingListSyncRemove.as_str().to_string(),
                user_id: Some(user.id),
                actor_id: None,
                event_data: serde_json::json!({
                    "email": user.email,
                    "reason": "webhook_unsubscribe",
                }),
                ip_address: None,
                user_agent: None,
            };
            if let Err(e) = state.db.create_audit_log(&log) {
                tracing::error!("Failed to write Groups.io webhook opt-out audit: {e}");
            }
            Ok(Json(ApiResponse::success(WebhookAck { handled: true })))
        }
        Ok(None) => Ok(Json(ApiResponse::success(WebhookAck { handled: false }))),
        Err(e) => Err(ApiError::from(e)),
    }
}

/// Hex-encoded HMAC-SHA256 of `body` keyed by `secret`.
fn sign_body(secret: &str, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Verify an inbound signature. An empty secret verifies nothing (the caller
/// refuses those before reaching here). The `sha256=` prefix is optional.
fn verify_signature(secret: &str, body: &[u8], provided: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    let provided = provided.strip_prefix("sha256=").unwrap_or(provided);
    constant_time_eq(sign_body(secret, body).as_bytes(), provided.as_bytes())
}

/// Length-checked constant-time byte comparison, so a wrong signature cannot be
/// distinguished from a right one by timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Whether a webhook body describes a member leaving/unsubscribing/being
/// removed. Deliberately generous across the action-word field, because the
/// exact Groups.io vocabulary is settled against a live account and the cost of
/// a false negative (missed here, caught by the poll) is lower than a false
/// positive.
fn event_indicates_removal(body: &serde_json::Value) -> bool {
    for key in ["action", "type", "event"] {
        if let Some(s) = body.get(key).and_then(|v| v.as_str()) {
            let s = s.to_ascii_lowercase();
            if ["leav", "remov", "unsub", "ban", "delete"]
                .iter()
                .any(|needle| s.contains(needle))
            {
                return true;
            }
        }
    }
    false
}

/// Pull the affected address out of a webhook body, checking the common nesting.
fn extract_email(body: &serde_json::Value) -> Option<String> {
    if let Some(e) = body.get("email").and_then(|v| v.as_str()) {
        return Some(e.to_string());
    }
    for parent in ["member", "sub", "member_info", "extra"] {
        if let Some(e) = body
            .get(parent)
            .and_then(|p| p.get("email"))
            .and_then(|v| v.as_str())
        {
            return Some(e.to_string());
        }
    }
    None
}

/// The address to opt out, if this body is a removal/unsubscribe we can act on.
fn parse_membership_removal(body: &serde_json::Value) -> Option<String> {
    if event_indicates_removal(body) {
        extract_email(body)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_correct_signature_verifies() {
        let secret = "s3cr3t";
        let body = br#"{"action":"leave","email":"a@x.org"}"#;
        let sig = sign_body(secret, body);
        assert!(verify_signature(secret, body, &sig));
        assert!(verify_signature(secret, body, &format!("sha256={sig}")));
    }

    #[test]
    fn a_wrong_or_absent_signature_is_rejected() {
        let body = br#"{"action":"leave"}"#;
        assert!(!verify_signature("s3cr3t", body, "deadbeef"));
        assert!(!verify_signature("s3cr3t", body, ""));
        // A different secret must not verify.
        let sig = sign_body("other", body);
        assert!(!verify_signature("s3cr3t", body, &sig));
    }

    #[test]
    fn an_empty_secret_verifies_nothing() {
        let body = br#"{}"#;
        assert!(!verify_signature("", body, &sign_body("", body)));
    }

    #[test]
    fn a_removal_event_yields_the_email() {
        assert_eq!(
            parse_membership_removal(&json!({"action": "leave", "email": "a@x.org"})),
            Some("a@x.org".to_string())
        );
        assert_eq!(
            parse_membership_removal(
                &json!({"type": "member_removed", "member": {"email": "b@x.org"}})
            ),
            Some("b@x.org".to_string())
        );
    }

    #[test]
    fn a_non_removal_event_is_ignored() {
        assert_eq!(
            parse_membership_removal(&json!({"action": "join", "email": "a@x.org"})),
            None
        );
        assert_eq!(
            parse_membership_removal(&json!({"action": "new_message"})),
            None
        );
    }
}
