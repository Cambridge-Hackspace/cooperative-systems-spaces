//! Stripe HTTP surface for the membership module.
//!
//! Two authenticated endpoints mint Stripe-hosted URLs (Checkout to start a
//! membership, Billing Portal to manage/cancel/resume), and one public,
//! signature-verified webhook turns Stripe payment events into ledger credits
//! and role changes. No card data ever reaches the platform: the SPA only
//! redirects to the hosted URLs, and the webhook carries amounts and reference
//! ids, not card details (SAQ-A posture).
//!
//! The webhook mirrors the groups.io endpoint's shape: **404 when the module is
//! disabled** (a 403 on an unguarded route reads as an auth gate the contract
//! matrix rejects), **403 when no webhook secret is set**, and **401 on a bad or
//! stale signature**. Verification follows Stripe's scheme: the `Stripe-Signature`
//! header is `t=<unix>,v1=<hex>`, and the signed payload is `"{t}.{raw body}"`
//! with a timestamp tolerance window.

use axum::{body::Bytes, extract::State, http::HeaderMap, response::Json, routing::post, Router};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::AuthUser,
    membership::decimal_to_minor_units,
    models::{AuditEventType, LedgerEntryType, NewAuditLog, User},
    stripe::{CheckoutMode, StripeClient},
    AppState,
};

type HmacSha256 = Hmac<Sha256>;

const SIGNATURE_HEADER: &str = "stripe-signature";
/// How far the signature timestamp may be from now, in seconds (Stripe's own
/// default tolerance). Rejects replayed captures of an old signed body.
const SIGNATURE_TOLERANCE_SECS: i64 = 300;

/// Refuse an authenticated Stripe endpoint when Stripe is switched off.
fn require_stripe_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().stripe.enabled {
        return Err(ApiError::Forbidden(
            "Stripe integration is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/checkout", post(create_checkout))
        .route("/portal", post(create_portal))
        .route("/webhook", post(receive_webhook))
}

/// What kind of checkout the member asked for.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutModeRequest {
    Subscription,
    OneShot,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    pub mode: CheckoutModeRequest,
}

#[derive(Debug, Serialize)]
pub struct RedirectResponse {
    pub url: String,
}

/// POST /api/stripe/checkout -- create a hosted Checkout Session for the member.
async fn create_checkout(
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CheckoutRequest>,
) -> Result<Json<ApiResponse<RedirectResponse>>, ApiError> {
    require_stripe_enabled(&state)?;
    let u = &user.0;
    let cfg = state.config_manager.get_config();
    let client = StripeClient::new(state.config_manager.clone());

    let mode = match req.mode {
        CheckoutModeRequest::Subscription => CheckoutMode::Subscription,
        CheckoutModeRequest::OneShot => {
            // One-shot buys `one_shot_periods` periods at the configured dues.
            use std::str::FromStr;
            let due = bigdecimal::BigDecimal::from_str(cfg.membership.due_amount.trim()).map_err(
                |_| {
                    ApiError::InternalServerError(
                        "membership.due_amount is not a valid decimal".to_string(),
                    )
                },
            )?;
            let periods = cfg.membership.one_shot_periods.max(1);
            let total = due * bigdecimal::BigDecimal::from(periods);
            CheckoutMode::OneShot {
                amount_cents: decimal_to_minor_units(&total),
            }
        }
    };

    let url = client
        .create_checkout_session(
            mode,
            &u.email,
            &u.id.to_string(),
            u.stripe_customer_id.as_deref(),
        )
        .await
        .map_err(|e| ApiError::InternalServerError(format!("Stripe checkout failed: {e}")))?;

    Ok(Json(ApiResponse::success(RedirectResponse { url })))
}

/// POST /api/stripe/portal -- open the Billing Portal for the member.
async fn create_portal(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<RedirectResponse>>, ApiError> {
    require_stripe_enabled(&state)?;
    let u = &user.0;
    let customer = u.stripe_customer_id.clone().ok_or_else(|| {
        ApiError::BadRequest(
            "No Stripe customer for this account yet; start a membership first".to_string(),
        )
    })?;
    let client = StripeClient::new(state.config_manager.clone());
    let url = client
        .create_billing_portal_session(&customer)
        .await
        .map_err(|e| ApiError::InternalServerError(format!("Stripe portal failed: {e}")))?;
    Ok(Json(ApiResponse::success(RedirectResponse { url })))
}

/// Acknowledgement for an inbound webhook: whether it moved anything.
#[derive(Debug, Serialize)]
pub struct WebhookAck {
    pub handled: bool,
}

/// POST /api/stripe/webhook -- inbound Stripe event.
async fn receive_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ApiResponse<WebhookAck>>, ApiError> {
    // Disabled module: answer "not here" (404), not "forbidden" (403) -- see the
    // groups.io webhook for why a public route must not read as auth-gated.
    if !state.config_manager.get_config().stripe.enabled {
        return Err(ApiError::NotFound(
            "Stripe integration is not enabled".to_string(),
        ));
    }
    let secret = state.config_manager.get_config().stripe.webhook_secret;
    if secret.trim().is_empty() {
        return Err(ApiError::Forbidden(
            "Stripe inbound webhook is not configured (no webhook_secret)".to_string(),
        ));
    }

    let provided = headers
        .get(SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let now = chrono::Utc::now().timestamp();
    if !verify_stripe_signature(&secret, &body, provided, now, SIGNATURE_TOLERANCE_SECS) {
        return Err(ApiError::Unauthorized(
            "Invalid Stripe webhook signature".to_string(),
        ));
    }

    let event: StripeEvent = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("Webhook body is not a Stripe event".to_string()))?;
    let obj = &event.data.object;

    let handled = match event.event_type.as_str() {
        "checkout.session.completed" => handle_checkout_completed(&state, obj)?,
        "invoice.paid" | "invoice.payment_succeeded" => handle_invoice_paid(&state, obj)?,
        "invoice.payment_failed" => handle_payment_failed(&state, obj)?,
        "customer.subscription.updated" => handle_subscription_updated(&state, obj)?,
        "customer.subscription.deleted" => handle_subscription_deleted(&state, obj)?,
        "charge.refunded" => handle_charge_refunded(&state, obj)?,
        // Any other event type is acknowledged and ignored (Stripe retries only
        // on non-2xx, so a 200 stops the retries for events we do not act on).
        _ => false,
    };

    Ok(Json(ApiResponse::success(WebhookAck { handled })))
}

/// The membership service, or a 500 if the module is enabled but unwired.
fn service(
    state: &AppState,
) -> Result<std::sync::Arc<crate::membership::MembershipService>, ApiError> {
    state.membership.clone().ok_or_else(|| {
        ApiError::InternalServerError("Membership service is not running".to_string())
    })
}

/// `checkout.session.completed`: link the customer/subscription to the user
/// (identified by `client_reference_id`), and for a one-shot payment post the
/// credit now (a subscription's money arrives via `invoice.paid`).
fn handle_checkout_completed(state: &AppState, obj: &serde_json::Value) -> Result<bool, ApiError> {
    let Some(user) = user_from_client_reference(state, obj)? else {
        return Ok(false);
    };
    if let Some(customer) = str_field(obj, "customer") {
        state
            .db
            .set_stripe_customer_id(user.id, Some(customer.as_str()))
            .map_err(ApiError::from)?;
    }
    let subscription = str_field(obj, "subscription");
    if let Some(sub) = subscription.as_deref() {
        state
            .db
            .set_stripe_subscription(user.id, Some(sub), Some("active"))
            .map_err(ApiError::from)?;
        audit(
            state,
            AuditEventType::SubscriptionStarted,
            user.id,
            serde_json::json!({}),
        );
    }

    let mode = str_field(obj, "mode");
    if mode.as_deref() == Some("payment") {
        // One-shot: the money is this session; credit it, keyed on the session id.
        if let Some(amount) = obj.get("amount_total").and_then(|v| v.as_i64()) {
            let reference = str_field(obj, "id");
            // Re-load so any customer id we just set is visible to the credit.
            let user = state
                .db
                .find_user_by_id(user.id)
                .map_err(ApiError::from)?
                .unwrap_or(user);
            service(state)?
                .record_credit(
                    &user,
                    LedgerEntryType::StripePayment,
                    crate::membership::minor_units_to_decimal(amount),
                    reference,
                    Some("Stripe one-time payment".to_string()),
                    None,
                )
                .map_err(ApiError::from)?;
        }
    }
    Ok(true)
}

/// `invoice.paid`: credit the amount, keyed on the invoice id for idempotency.
fn handle_invoice_paid(state: &AppState, obj: &serde_json::Value) -> Result<bool, ApiError> {
    let Some(customer) = str_field(obj, "customer") else {
        return Ok(false);
    };
    let Some(user) = state
        .db
        .find_user_by_stripe_customer_id(&customer)
        .map_err(ApiError::from)?
    else {
        return Ok(false);
    };
    let amount = obj.get("amount_paid").and_then(|v| v.as_i64()).unwrap_or(0);
    let reference = str_field(obj, "id");
    service(state)?
        .record_credit(
            &user,
            LedgerEntryType::StripePayment,
            crate::membership::minor_units_to_decimal(amount),
            reference,
            Some("Stripe invoice".to_string()),
            None,
        )
        .map_err(ApiError::from)?;
    Ok(true)
}

/// `invoice.payment_failed`: record it for visibility. The lapse itself happens
/// through the balance check, not this event.
fn handle_payment_failed(state: &AppState, obj: &serde_json::Value) -> Result<bool, ApiError> {
    let Some(customer) = str_field(obj, "customer") else {
        return Ok(false);
    };
    let Some(user) = state
        .db
        .find_user_by_stripe_customer_id(&customer)
        .map_err(ApiError::from)?
    else {
        return Ok(false);
    };
    audit(
        state,
        AuditEventType::SubscriptionPaymentFailed,
        user.id,
        serde_json::json!({}),
    );
    Ok(true)
}

/// `customer.subscription.updated`: store the latest subscription id + status.
fn handle_subscription_updated(
    state: &AppState,
    obj: &serde_json::Value,
) -> Result<bool, ApiError> {
    let Some(customer) = str_field(obj, "customer") else {
        return Ok(false);
    };
    let Some(user) = state
        .db
        .find_user_by_stripe_customer_id(&customer)
        .map_err(ApiError::from)?
    else {
        return Ok(false);
    };
    let sub = str_field(obj, "id");
    let status = str_field(obj, "status");
    state
        .db
        .set_stripe_subscription(user.id, sub.as_deref(), status.as_deref())
        .map_err(ApiError::from)?;
    Ok(true)
}

/// `customer.subscription.deleted`: clear the subscription id, stamp canceled.
/// The lapse follows from the balance check once dues stop being credited.
fn handle_subscription_deleted(
    state: &AppState,
    obj: &serde_json::Value,
) -> Result<bool, ApiError> {
    let Some(customer) = str_field(obj, "customer") else {
        return Ok(false);
    };
    let Some(user) = state
        .db
        .find_user_by_stripe_customer_id(&customer)
        .map_err(ApiError::from)?
    else {
        return Ok(false);
    };
    state
        .db
        .set_stripe_subscription(user.id, None, Some("canceled"))
        .map_err(ApiError::from)?;
    audit(
        state,
        AuditEventType::SubscriptionCanceled,
        user.id,
        serde_json::json!({}),
    );
    Ok(true)
}

/// `charge.refunded`: post a refund debit, keyed on the charge id.
fn handle_charge_refunded(state: &AppState, obj: &serde_json::Value) -> Result<bool, ApiError> {
    let Some(customer) = str_field(obj, "customer") else {
        return Ok(false);
    };
    let Some(user) = state
        .db
        .find_user_by_stripe_customer_id(&customer)
        .map_err(ApiError::from)?
    else {
        return Ok(false);
    };
    let refunded = obj
        .get("amount_refunded")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if refunded <= 0 {
        return Ok(false);
    }
    // Negative: a refund reduces available credit. Keyed on the charge id so a
    // redelivered refund event posts once.
    let reference = str_field(obj, "id").map(|id| format!("{id}:refund"));
    service(state)?
        .record_credit(
            &user,
            LedgerEntryType::StripeRefund,
            -crate::membership::minor_units_to_decimal(refunded),
            reference,
            Some("Stripe refund".to_string()),
            None,
        )
        .map_err(ApiError::from)?;
    Ok(true)
}

/// Resolve the user a checkout session names via `client_reference_id`.
fn user_from_client_reference(
    state: &AppState,
    obj: &serde_json::Value,
) -> Result<Option<User>, ApiError> {
    let Some(reference) = str_field(obj, "client_reference_id") else {
        return Ok(None);
    };
    let Ok(uid) = uuid::Uuid::parse_str(&reference) else {
        return Ok(None);
    };
    state.db.find_user_by_id(uid).map_err(ApiError::from)
}

/// A string field on a JSON object, if present and a string.
fn str_field(obj: &serde_json::Value, key: &str) -> Option<String> {
    obj.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Best-effort audit write.
fn audit(state: &AppState, event: AuditEventType, user_id: uuid::Uuid, data: serde_json::Value) {
    let log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: Some(user_id),
        actor_id: None,
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write Stripe audit {}: {e}", event.as_str());
    }
}

#[derive(Debug, Deserialize)]
struct StripeEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: StripeEventData,
}

#[derive(Debug, Deserialize)]
struct StripeEventData {
    #[serde(default)]
    object: serde_json::Value,
}

/// Hex HMAC-SHA256 of Stripe's signed payload `"{timestamp}.{body}"`.
fn sign_stripe(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Verify a `Stripe-Signature` header (`t=<unix>,v1=<hex>[,v1=<hex>...]`).
///
/// Rejects an empty secret, a missing `t`/`v1`, a timestamp outside `tolerance`
/// seconds of `now`, and a `v1` that does not match the HMAC of `"{t}.{body}"`.
/// The comparison is constant-time.
fn verify_stripe_signature(
    secret: &str,
    body: &[u8],
    header: &str,
    now: i64,
    tolerance: i64,
) -> bool {
    if secret.is_empty() {
        return false;
    }
    let mut timestamp: Option<i64> = None;
    let mut signatures: Vec<&str> = Vec::new();
    for part in header.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.trim() {
            "t" => timestamp = v.trim().parse::<i64>().ok(),
            "v1" => signatures.push(v.trim()),
            _ => {}
        }
    }
    let Some(t) = timestamp else {
        return false;
    };
    if (now - t).abs() > tolerance {
        return false;
    }
    if signatures.is_empty() {
        return false;
    }
    let expected = sign_stripe(secret, t, body);
    signatures
        .iter()
        .any(|s| constant_time_eq(expected.as_bytes(), s.as_bytes()))
}

/// Length-checked constant-time byte comparison.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn header_for(secret: &str, t: i64, body: &[u8]) -> String {
        format!("t={t},v1={}", sign_stripe(secret, t, body))
    }

    #[test]
    fn a_correct_signature_within_tolerance_verifies() {
        let secret = "whsec_test";
        let body = br#"{"type":"invoice.paid"}"#;
        let t = 1_000_000;
        let header = header_for(secret, t, body);
        assert!(verify_stripe_signature(secret, body, &header, t + 10, 300));
    }

    #[test]
    fn a_wrong_v1_is_rejected() {
        let body = br#"{"type":"invoice.paid"}"#;
        let t = 1_000_000;
        let header = format!("t={t},v1=deadbeef");
        assert!(!verify_stripe_signature(
            "whsec_test",
            body,
            &header,
            t,
            300
        ));
    }

    #[test]
    fn a_tampered_body_is_rejected() {
        let secret = "whsec_test";
        let t = 1_000_000;
        let header = header_for(secret, t, br#"{"amount_paid":1000}"#);
        // Same signature, different body: must not verify.
        assert!(!verify_stripe_signature(
            secret,
            br#"{"amount_paid":999999}"#,
            &header,
            t,
            300
        ));
    }

    #[test]
    fn a_stale_timestamp_is_rejected() {
        let secret = "whsec_test";
        let body = br#"{}"#;
        let t = 1_000_000;
        let header = header_for(secret, t, body);
        // now is well beyond the tolerance window: a replayed capture.
        assert!(!verify_stripe_signature(
            secret,
            body,
            &header,
            t + 10_000,
            300
        ));
    }

    #[test]
    fn an_absent_or_malformed_header_is_rejected() {
        let body = br#"{}"#;
        assert!(!verify_stripe_signature("whsec_test", body, "", 0, 300));
        assert!(!verify_stripe_signature(
            "whsec_test",
            body,
            "v1=abc",
            0,
            300
        ));
        assert!(!verify_stripe_signature(
            "whsec_test",
            body,
            "t=1",
            body_sig_missing(),
            300
        ));
    }

    // A tiny helper so the "no v1" case reads clearly above.
    fn body_sig_missing() -> i64 {
        1
    }

    #[test]
    fn an_empty_secret_verifies_nothing() {
        let body = br#"{}"#;
        let header = header_for("", 1, body);
        assert!(!verify_stripe_signature("", body, &header, 1, 300));
    }
}
