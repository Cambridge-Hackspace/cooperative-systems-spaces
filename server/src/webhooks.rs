//! Outbound webhook dispatch.
//!
//! New audit logs are forwarded (via an unbounded channel set on the
//! [`DatabaseManager`]) to a background consumer here. For each enabled webhook
//! subscribed to the event's type we build a signed JSON request, attach any
//! reusable auth headers, POST it with bounded retries, and record every
//! attempt in `webhook_deliveries`.

use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, Mac};
use rand::distributions::Alphanumeric;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::json;
use sha2::Sha256;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::config::ConfigManager;
use crate::database::DatabaseManager;
use crate::models::{AuditLog, NewWebhookDelivery, Webhook, WebhookAuthHeader};

type HmacSha256 = Hmac<Sha256>;

/// Maximum number of delivery attempts per webhook per event.
const MAX_ATTEMPTS: u32 = 3;
/// Per-request HTTP timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Generate a random per-webhook signing secret.
pub fn generate_signing_secret() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

/// Hex-encoded HMAC-SHA256 of `body` keyed by `secret`.
fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

#[derive(Clone)]
pub struct WebhookDispatcher {
    db: Arc<DatabaseManager>,
    http: reqwest::Client,
    /// Read live rather than snapshotted, so `admin_config_reload` takes effect
    /// on the next delivery instead of at the next restart.
    config: Arc<ConfigManager>,
}

impl WebhookDispatcher {
    /// Build the dispatcher and spawn its background consumer, returning the
    /// sender to register on the [`DatabaseManager`].
    pub fn start(
        db: Arc<DatabaseManager>,
        config: Arc<ConfigManager>,
    ) -> (Arc<Self>, mpsc::UnboundedSender<AuditLog>) {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                warn!("Failed to build webhook HTTP client ({e}); using default");
                reqwest::Client::new()
            });

        let dispatcher = Arc::new(Self { db, http, config });
        let (tx, mut rx) = mpsc::unbounded_channel::<AuditLog>();

        let consumer = dispatcher.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let d = consumer.clone();
                // One task per event so a slow endpoint can't block others.
                tokio::spawn(async move { d.handle_event(event).await });
            }
            debug!("Webhook dispatch channel closed; consumer stopping");
        });

        (dispatcher, tx)
    }

    /// Look up matching webhooks for an audit event and deliver to each.
    async fn handle_event(&self, event: AuditLog) {
        let webhooks = match self.db.get_enabled_webhooks_for_event(&event.event_type) {
            Ok(w) => w,
            Err(e) => {
                error!(
                    "Failed to load webhooks for event '{}': {}",
                    event.event_type, e
                );
                return;
            }
        };

        if webhooks.is_empty() {
            return;
        }

        let payload = event_payload(&event);
        for webhook in webhooks {
            // Outcome is already logged and persisted by `deliver`.
            let _ = self
                .deliver(&webhook, &payload, &event.event_type, Some(event.id))
                .await;
        }
    }

    /// Deliver one payload to one webhook, retrying on failure. Returns the
    /// final outcome and records every attempt.
    pub async fn deliver(
        &self,
        webhook: &Webhook,
        payload: &serde_json::Value,
        event_type: &str,
        audit_log_id: Option<Uuid>,
    ) -> Result<(), String> {
        let body = match serde_json::to_vec(payload) {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to serialize webhook payload: {}", e);
                return Err(format!("serialize payload: {e}"));
            }
        };
        let signature = sign(&webhook.signing_secret, &body);

        // Build the static header set once; reused across attempts.
        let headers = self.build_headers(webhook, event_type, &signature);

        let mut last_err = String::from("no attempts made");
        for attempt in 1..=MAX_ATTEMPTS {
            let result = self
                .http
                .post(&webhook.url)
                .headers(headers.clone())
                .body(body.clone())
                .send()
                .await;

            let (success, status_code, response_body, error) = match result {
                Ok(resp) => {
                    let status = resp.status();
                    let code = status.as_u16() as i32;
                    let text = resp.text().await.unwrap_or_default();
                    let debug = self.config.get_config().site.debug;
                    let body = recordable_body(debug, status.is_success(), &text);
                    if status.is_success() {
                        (true, Some(code), body, None)
                    } else {
                        (
                            false,
                            Some(code),
                            body,
                            Some(format!("non-success status {code}")),
                        )
                    }
                }
                Err(e) => (false, None, None, Some(e.to_string())),
            };

            self.record(NewWebhookDelivery {
                webhook_id: webhook.id,
                audit_log_id,
                event_type: event_type.to_string(),
                attempt: attempt as i32,
                success,
                status_code,
                response_body,
                error: error.clone(),
                request_payload: Some(payload.clone()),
            });

            if success {
                return Ok(());
            }

            last_err = error.unwrap_or_else(|| "unknown error".to_string());
            warn!(
                "Webhook '{}' delivery attempt {}/{} failed: {}",
                webhook.name, attempt, MAX_ATTEMPTS, last_err
            );

            if attempt < MAX_ATTEMPTS {
                // Exponential backoff: 1s, 2s, ...
                let backoff = Duration::from_secs(1 << (attempt - 1));
                tokio::time::sleep(backoff).await;
            }
        }

        Err(last_err)
    }

    /// Assemble request headers: content type, metadata, signature, and the
    /// webhook's reusable auth headers (secret values loaded here only).
    fn build_headers(&self, webhook: &Webhook, event_type: &str, signature: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if let Ok(v) = HeaderValue::from_str(&format!("sha256={signature}")) {
            headers.insert("X-Webhook-Signature", v);
        }
        if let Ok(v) = HeaderValue::from_str(event_type) {
            headers.insert("X-Webhook-Event", v);
        }
        if let Ok(v) = HeaderValue::from_str(&webhook.id.to_string()) {
            headers.insert("X-Webhook-Id", v);
        }

        match self.db.get_webhook_auth_headers_for_dispatch(webhook.id) {
            Ok(auth_headers) => {
                for h in auth_headers {
                    apply_auth_header(&mut headers, &h);
                }
            }
            Err(e) => warn!(
                "Failed to load auth headers for webhook '{}': {}",
                webhook.name, e
            ),
        }

        headers
    }

    fn record(&self, delivery: NewWebhookDelivery) {
        if let Err(e) = self.db.record_webhook_delivery(&delivery) {
            error!("Failed to record webhook delivery: {}", e);
        }
    }
}

/// Add a single reusable auth header, logging and skipping malformed names/values.
fn apply_auth_header(headers: &mut HeaderMap, h: &WebhookAuthHeader) {
    match (
        HeaderName::from_bytes(h.header_name.as_bytes()),
        HeaderValue::from_str(&h.header_value),
    ) {
        (Ok(name), Ok(value)) => {
            headers.insert(name, value);
        }
        _ => warn!(
            "Skipping invalid auth header '{}' ({})",
            h.name, h.header_name
        ),
    }
}

/// Canonical JSON body sent to webhook receivers.
pub fn event_payload(event: &AuditLog) -> serde_json::Value {
    json!({
        "event": {
            "id": event.id,
            "event_type": event.event_type,
            "user_id": event.user_id,
            "actor_id": event.actor_id,
            "data": event.event_data,
            "ip_address": event.ip_address,
            "user_agent": event.user_agent,
            "created_at": event.created_at,
        },
        "delivered_at": chrono::Utc::now(),
    })
}

/// A synthetic payload used by the "test" endpoint.
pub fn test_payload() -> serde_json::Value {
    json!({
        "event": {
            "id": Uuid::nil(),
            "event_type": "webhook_test",
            "data": { "message": "This is a test webhook delivery." },
            "created_at": chrono::Utc::now(),
        },
        "delivered_at": chrono::Utc::now(),
        "test": true,
    })
}

/// What, if anything, of a receiver's response to keep.
///
/// The body is a debugging aid: when a delivery fails, the receiver's own words
/// are usually the only thing that says why -- "invalid token", "unknown room",
/// "payload too large". Without it the Deliveries tab shows a bare status code
/// and the operator is guessing.
///
/// It is also, on a *successful* delivery, whatever the URL returned. Nothing
/// validates a webhook's host beyond its scheme (api/webhooks.rs), so an admin
/// can point one at a link-local or loopback address, fire a test delivery, and
/// read the answer back out of the admin UI. That is a read primitive against
/// anything the server can reach, built out of a debugging convenience.
///
/// So the two are separated. A failure keeps its body, because that is the case
/// the field exists for and a failing delivery is the one worth diagnosing. A
/// success keeps nothing unless `[site] debug` is on -- which is off by default
/// and is already the flag for "this deployment is being worked on rather than
/// run".
pub fn recordable_body(debug: bool, success: bool, text: &str) -> Option<String> {
    if success && !debug {
        return None;
    }
    Some(truncate(text, 4096))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Back off to the nearest char boundary at or below `max`.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::recordable_body;

    // The debugging value: a failed delivery keeps the receiver's own words,
    // because that is the case the field exists for and the only one an
    // operator is trying to diagnose.
    #[test]
    fn a_failed_delivery_keeps_its_body_whatever_debug_says() {
        assert_eq!(
            recordable_body(false, false, "invalid token"),
            Some("invalid token".to_string())
        );
        assert_eq!(
            recordable_body(true, false, "invalid token"),
            Some("invalid token".to_string())
        );
    }

    // The exposure: nothing validates a webhook's host beyond its scheme, so a
    // *successful* body is whatever the URL returned -- including a link-local
    // metadata endpoint. Off by default, it is not kept.
    #[test]
    fn a_successful_delivery_keeps_nothing_unless_debug_is_on() {
        assert_eq!(
            recordable_body(false, true, "ami-id\ninstance-id\niam/"),
            None
        );
    }

    #[test]
    fn a_successful_delivery_keeps_its_body_when_debug_is_on() {
        assert_eq!(
            recordable_body(true, true, "queued"),
            Some("queued".to_string())
        );
    }

    // The cap still applies wherever a body is kept: a receiver that answers
    // with a megabyte of HTML should not put a megabyte in every delivery row.
    #[test]
    fn a_kept_body_is_still_truncated() {
        let huge = "x".repeat(10_000);
        let kept = recordable_body(false, false, &huge).expect("a failure keeps its body");
        assert!(kept.len() < huge.len());
        assert!(kept.ends_with("…[truncated]"));
    }

    #[test]
    fn truncation_does_not_split_a_character() {
        // 4096 is a byte cap and the boundary lands mid-character here, which
        // is a panic rather than a wrong answer if it is not handled.
        let body = "é".repeat(4000);
        let kept = recordable_body(true, false, &body).expect("kept");
        assert!(kept.ends_with("…[truncated]"));
    }
}
