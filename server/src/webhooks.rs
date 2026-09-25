//! Outbound webhook dispatch.
//!
//! New audit logs are forwarded (via an unbounded channel set on the
//! [`DatabaseManager`]) to a background consumer here. For each enabled webhook
//! subscribed to the event's type we build a signed JSON request, attach any
//! reusable auth headers, POST it with bounded retries, and record every
//! attempt in `webhook_deliveries`.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};
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
/// AWS IMDSv6 metadata address (mirrors `api::webhooks`). fd00:ec2::254.
const AWS_IMDS_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254);
/// Cap on how much of a receiver's response we read into memory. Only 4096
/// bytes are ever stored (see `recordable_body`); this bounds the *read* so a
/// receiver answering with gigabytes cannot exhaust memory before we truncate.
const MAX_RESPONSE_READ: usize = 64 * 1024;

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
        shutdown: crate::shutdown::Shutdown,
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
        let consumer_shutdown = shutdown.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    maybe = rx.recv() => match maybe {
                        Some(event) => {
                            let d = consumer.clone();
                            // One task per event so a slow endpoint can't block
                            // others. Tracked, so shutdown waits for a delivery
                            // that is already in flight (#102).
                            consumer_shutdown.spawn(async move { d.handle_event(event).await });
                        }
                        None => {
                            debug!("Webhook dispatch channel closed; consumer stopping");
                            return;
                        }
                    },
                    _ = consumer_shutdown.cancelled() => break,
                }
            }

            // Cancelled with events still queued. Those are audit events that
            // were accepted and told they would be delivered, so drain what is
            // already in hand before stopping -- the deliveries themselves are
            // tracked and bounded by the drain deadline.
            //
            // `try_recv` rather than `recv`: the senders are alive for the
            // lifetime of the process, so awaiting would block until the
            // deadline on every shutdown instead of finishing immediately.
            let mut drained = 0usize;
            while let Ok(event) = rx.try_recv() {
                let d = consumer.clone();
                consumer_shutdown.spawn(async move { d.handle_event(event).await });
                drained += 1;
            }
            if drained > 0 {
                debug!("Webhook consumer drained {drained} queued event(s) at shutdown");
            }
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

        // #120 (#16): resolve the host and screen every address it maps to
        // *before* sending, refusing anything that resolves to a link-local or
        // cloud-metadata address. `validate_url` at creation only inspects the
        // URL literal, so a hostname whose A record is 169.254.169.254 slips
        // past it; only resolution catches that. On refusal we record one failed
        // attempt (so the admin sees why) and stop.
        let (host, resolved) = match resolve_and_screen(&webhook.url).await {
            Ok(v) => v,
            Err(reason) => {
                self.record(NewWebhookDelivery {
                    webhook_id: webhook.id,
                    audit_log_id,
                    event_type: event_type.to_string(),
                    attempt: 1,
                    success: false,
                    status_code: None,
                    response_body: None,
                    error: Some(reason.clone()),
                    request_payload: Some(payload.clone()),
                });
                warn!("Webhook '{}' delivery refused: {}", webhook.name, reason);
                return Err(reason);
            }
        };

        // Pin the client to the exact addresses we screened, so reqwest does not
        // re-resolve at connect time and land on an address we rejected (the
        // DNS-rebinding gap the literal check cannot close).
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .resolve_to_addrs(&host, &resolved)
            .build()
            .unwrap_or_else(|e| {
                warn!("Failed to build pinned webhook client ({e}); using shared client");
                self.http.clone()
            });

        let mut last_err = String::from("no attempts made");
        for attempt in 1..=MAX_ATTEMPTS {
            let result = client
                .post(&webhook.url)
                .headers(headers.clone())
                .body(body.clone())
                .send()
                .await;

            let (success, status_code, response_body, error) = match result {
                Ok(resp) => {
                    let status = resp.status();
                    let code = status.as_u16() as i32;
                    let text = read_body_capped(resp).await;
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

/// True for an IP a webhook must never reach: link-local (v4 169.254.0.0/16,
/// v6 fe80::/10) and the cloud-metadata addresses that live there. Mirrors
/// `api::webhooks::is_link_local`, but over a *resolved* `IpAddr` rather than a
/// URL literal -- the whole point of #120 (#16). RFC1918 and loopback stay
/// permitted, deliberately and consistently with the literal check: a hackspace
/// webhook legitimately addresses a box on its own LAN.
fn ip_is_forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => {
            // A v4-mapped address (::ffff:a.b.c.d) is really an IPv4 destination;
            // screen it as one so ::ffff:169.254.169.254 cannot sneak through.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4.is_link_local();
            }
            (v6.segments()[0] & 0xffc0) == 0xfe80 || v6 == AWS_IMDS_V6
        }
    }
}

/// Resolve the webhook host and confirm every address it maps to is permitted,
/// returning the host and the screened socket addresses to pin the client to.
/// Returns `Err(reason)` when the host is missing, does not resolve, or resolves
/// (even in part) to a forbidden address -- refusing to send rather than letting
/// reqwest re-resolve at connect time.
async fn resolve_and_screen(url: &str) -> Result<(String, Vec<SocketAddr>), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "URL has no known port".to_string())?;

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| format!("could not resolve {host}: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("{host} resolved to no addresses"));
    }
    if let Some(bad) = addrs.iter().find(|a| ip_is_forbidden(a.ip())) {
        return Err(format!(
            "{host} resolves to a blocked address ({}); refusing delivery",
            bad.ip()
        ));
    }
    Ok((host, addrs))
}

/// Read at most [`MAX_RESPONSE_READ`] bytes of a response body. The receiver is
/// untrusted and so is its `Content-Length`; `resp.text()` would buffer the
/// whole thing first. We only ever store 4096 bytes, so reading past a small
/// bound serves nothing and risks memory exhaustion.
async fn read_body_capped(mut resp: reqwest::Response) -> String {
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < MAX_RESPONSE_READ {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let room = MAX_RESPONSE_READ - buf.len();
                if chunk.len() <= room {
                    buf.extend_from_slice(&chunk);
                } else {
                    buf.extend_from_slice(&chunk[..room]);
                    break; // hit the cap mid-chunk; stop reading the rest
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
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

    // #120 (#16): the SSRF screen over a resolved address.
    mod ssrf_screen {
        use super::super::{ip_is_forbidden, resolve_and_screen};
        use std::net::IpAddr;

        #[test]
        fn resolved_metadata_and_link_local_addresses_are_refused() {
            for s in [
                "169.254.169.254",        // AWS/GCP/Azure IMDS (v4 link-local)
                "169.254.0.1",            // link-local generally
                "fe80::1",                // v6 link-local
                "fd00:ec2::254",          // AWS IMDSv6
                "::ffff:169.254.169.254", // v4-mapped link-local
            ] {
                let ip: IpAddr = s.parse().unwrap();
                assert!(ip_is_forbidden(ip), "{s} must be refused");
            }
        }

        #[test]
        fn resolved_lan_and_public_addresses_are_allowed() {
            // Consistent with validate_url's deliberate decision: LAN and
            // loopback destinations are a legitimate hackspace use case.
            for s in [
                "10.0.0.9",
                "192.168.1.50",
                "172.16.4.4",
                "127.0.0.1",
                "8.8.8.8",
                "fd12:3456::1",
            ] {
                let ip: IpAddr = s.parse().unwrap();
                assert!(!ip_is_forbidden(ip), "{s} must be allowed");
            }
        }

        // A URL whose *literal* host is a metadata address is refused at
        // resolution -- exercising resolve_and_screen end to end without needing
        // a controlled DNS server (an IP literal does not hit DNS). It does not,
        // by itself, prove the DNS-name case; that is covered by the fact that
        // resolution now runs at all plus the ip_is_forbidden oracle above.
        #[tokio::test]
        async fn a_metadata_literal_is_refused_at_resolution() {
            let err = resolve_and_screen("http://169.254.169.254/latest/meta-data/")
                .await
                .expect_err("must refuse a metadata address");
            assert!(err.contains("blocked"), "unexpected reason: {err}");
        }

        #[tokio::test]
        async fn a_lan_literal_resolves_and_screens_clean() {
            let (host, addrs) = resolve_and_screen("http://10.0.0.9:8080/hook")
                .await
                .expect("a LAN address is permitted");
            assert_eq!(host, "10.0.0.9");
            assert!(addrs.iter().all(|a| a.ip().to_string() == "10.0.0.9"));
        }
    }
}
