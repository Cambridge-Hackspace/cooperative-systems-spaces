use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::models::{
    AuditEventType, NewAuditLog, NewWebhook, NewWebhookAuthHeader, UpdateWebhook,
    UpdateWebhookAuthHeader, Webhook, WebhookAuthHeader, WebhookDelivery,
};
use crate::webhooks::generate_signing_secret;
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Auth header as exposed to clients — never includes the secret value.
#[derive(Debug, Serialize)]
pub struct AuthHeaderResponse {
    pub id: Uuid,
    pub name: String,
    pub header_name: String,
    /// Whether a secret value is stored. The value itself is write-only.
    pub has_value: bool,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl From<WebhookAuthHeader> for AuthHeaderResponse {
    fn from(h: WebhookAuthHeader) -> Self {
        Self {
            id: h.id,
            name: h.name,
            header_name: h.header_name,
            has_value: !h.header_value.is_empty(),
            created_at: h.created_at,
            updated_at: h.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateAuthHeaderRequest {
    pub name: String,
    pub header_name: String,
    pub header_value: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAuthHeaderRequest {
    pub name: Option<String>,
    pub header_name: Option<String>,
    /// When present, replaces the stored secret (write-only).
    pub header_value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    /// Per-webhook HMAC secret; shown to admins so they can verify signatures.
    pub signing_secret: String,
    pub event_types: Vec<String>,
    pub auth_header_ids: Vec<Uuid>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub name: String,
    pub url: String,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub auth_header_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    /// When present, replaces the full set of subscribed event types.
    pub event_types: Option<Vec<String>>,
    /// When present, replaces the full set of linked auth headers.
    pub auth_header_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct EventTypeInfo {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct DeliveryQuery {
    pub webhook_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

pub fn admin_webhook_routes() -> Router<AppState> {
    Router::new()
        // Auth headers (reusable write-only credentials)
        .route(
            "/auth-headers",
            post(create_auth_header).get(list_auth_headers),
        )
        .route(
            "/auth-headers/{id}",
            patch(update_auth_header).delete(delete_auth_header),
        )
        // Catalog + deliveries (static segments before /{id})
        .route("/event-types", get(list_event_types))
        .route("/deliveries", get(list_deliveries))
        // Webhooks
        .route("/", post(create_webhook).get(list_webhooks))
        .route(
            "/{id}",
            get(get_webhook)
                .patch(update_webhook)
                .delete(delete_webhook),
        )
        .route("/{id}/test", post(test_webhook))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn valid_event_types() -> HashSet<&'static str> {
    AuditEventType::all().iter().map(|e| e.as_str()).collect()
}

/// Reject unknown event types so a webhook can't subscribe to something that
/// will never fire.
fn validate_event_types(event_types: &[String]) -> Result<(), ApiError> {
    let valid = valid_event_types();
    for et in event_types {
        if !valid.contains(et.as_str()) {
            return Err(ApiError::BadRequest(format!("Unknown event type: {et}")));
        }
    }
    Ok(())
}

/// AWS publishes its IMDS over IPv6 at this address, which is a unique-local
/// address rather than a link-local one -- so a check that blocked only
/// `fe80::/10` would leave the exact target this exists to protect reachable.
const AWS_IMDS_V6: std::net::Ipv6Addr =
    std::net::Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254);

/// Is this address one of the cloud metadata endpoints, or link-local?
///
/// `169.254.169.254` is the metadata service on AWS, GCP, Azure, DigitalOcean
/// and Oracle. A single successful read there can return instance credentials,
/// so it is the one target where the difference between "an admin learned
/// something" and "an admin took over the account" is one request.
fn is_link_local(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(v4) => v4.is_link_local(),
        url::Host::Ipv6(v6) => {
            // `is_unicast_link_local` is unstable, so the `fe80::/10` test is
            // written out. The first ten bits are the prefix.
            let seg = v6.segments()[0];
            (seg & 0xffc0) == 0xfe80 || *v6 == AWS_IMDS_V6
        }
        url::Host::Domain(_) => false,
    }
}

/// Reject a webhook URL that is obviously not a webhook destination.
///
/// The scheme check is the old one: a `javascript:` or `file:` URL is never a
/// delivery target. What is new is the link-local block, and its scope is worth
/// being precise about, because this is a partial defence and pretending
/// otherwise would be worse than not having it.
///
/// **What it stops.** A literal link-local or cloud-metadata address. The
/// server fetches webhook URLs and records the outcome -- `status_code`,
/// `error`, and on a failure the response body -- back into a table the admin
/// UI displays. Pointing a webhook at `169.254.169.254` and reading the answer
/// out of the Deliveries tab is a credential read, not merely an information
/// leak.
///
/// **What it does not stop, deliberately.** RFC1918 and loopback are still
/// permitted. A hackspace webhook plausibly points at a Matrix server, a
/// printer or a Home Assistant box on the LAN, and blocking `10.0.0.0/8` would
/// break the primary use case to inconvenience an attacker who already holds an
/// admin account. Loopback is a narrower call and arguably should go too, but
/// it is a separate decision and is not being made silently here.
///
/// **What it cannot stop.** A hostname that *resolves* to a link-local address.
/// This inspects the literal in the URL; nothing re-checks after DNS, and a
/// name whose A record is 169.254.169.254 passes. Closing that needs the check
/// at connection time inside the HTTP client, which is a different and larger
/// change. This is a guard against the obvious, and the obvious is what gets
/// tried.
fn validate_url(url: &str) -> Result<(), ApiError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| ApiError::BadRequest("Webhook URL is not a valid URL".to_string()))?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ApiError::BadRequest(
            "Webhook URL must start with http:// or https://".to_string(),
        ));
    }

    match parsed.host() {
        Some(host) if is_link_local(&host) => Err(ApiError::BadRequest(
            "Webhook URL may not address a link-local or cloud metadata endpoint".to_string(),
        )),
        Some(_) => Ok(()),
        None => Err(ApiError::BadRequest(
            "Webhook URL must name a host".to_string(),
        )),
    }
}

/// Prettify an event type string into a human label (e.g. user_login -> "User Login").
fn humanize(value: &str) -> String {
    value
        .split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn to_webhook_response(state: &AppState, webhook: Webhook) -> Result<WebhookResponse, ApiError> {
    let event_types = state.db.get_webhook_event_types(webhook.id)?;
    let auth_header_ids = state.db.get_webhook_auth_header_ids(webhook.id)?;
    Ok(WebhookResponse {
        id: webhook.id,
        name: webhook.name,
        url: webhook.url,
        enabled: webhook.enabled,
        signing_secret: webhook.signing_secret,
        event_types,
        auth_header_ids,
        created_at: webhook.created_at,
        updated_at: webhook.updated_at,
    })
}

fn audit(state: &AppState, event: AuditEventType, actor: Uuid, data: serde_json::Value) {
    let log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: None,
        actor_id: Some(actor),
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write audit log for {}: {}", event.as_str(), e);
    }
}

// ---------------------------------------------------------------------------
// Auth header handlers
// ---------------------------------------------------------------------------

/// POST /api/admin/webhooks/auth-headers
async fn create_auth_header(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateAuthHeaderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.name.trim().is_empty() || req.header_name.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "name and header_name are required".to_string(),
        ));
    }
    if req.header_value.is_empty() {
        return Err(ApiError::BadRequest("header_value is required".to_string()));
    }

    let created = state.db.create_webhook_auth_header(&NewWebhookAuthHeader {
        name: req.name,
        header_name: req.header_name,
        header_value: req.header_value,
        created_by: Some(admin.0.id),
    })?;

    audit(
        &state,
        AuditEventType::WebhookAuthHeaderCreated,
        admin.0.id,
        serde_json::json!({ "auth_header_id": created.id, "name": created.name }),
    );

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(AuthHeaderResponse::from(created))),
    ))
}

/// GET /api/admin/webhooks/auth-headers
async fn list_auth_headers(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let headers = state.db.list_webhook_auth_headers()?;
    let resp: Vec<AuthHeaderResponse> = headers.into_iter().map(AuthHeaderResponse::from).collect();
    Ok(Json(ApiResponse::success(resp)))
}

/// PATCH /api/admin/webhooks/auth-headers/{id}
async fn update_auth_header(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateAuthHeaderRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(v) = &req.header_value {
        if v.is_empty() {
            return Err(ApiError::BadRequest(
                "header_value cannot be set to empty".to_string(),
            ));
        }
    }

    let updated = state.db.update_webhook_auth_header(
        id,
        &UpdateWebhookAuthHeader {
            name: req.name,
            header_name: req.header_name,
            header_value: req.header_value,
            updated_at: Some(Utc::now()),
        },
    )?;

    audit(
        &state,
        AuditEventType::WebhookAuthHeaderUpdated,
        admin.0.id,
        serde_json::json!({ "auth_header_id": id }),
    );

    Ok(Json(ApiResponse::success(AuthHeaderResponse::from(
        updated,
    ))))
}

/// DELETE /api/admin/webhooks/auth-headers/{id}
async fn delete_auth_header(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let usage = state.db.count_webhook_auth_header_usage(id)?;
    if usage > 0 {
        return Err(ApiError::Conflict(format!(
            "Auth header is still attached to {usage} webhook(s); detach it first"
        )));
    }

    let deleted = state.db.delete_webhook_auth_header(id)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Auth header not found".to_string()));
    }

    audit(
        &state,
        AuditEventType::WebhookAuthHeaderDeleted,
        admin.0.id,
        serde_json::json!({ "auth_header_id": id }),
    );

    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Auth header deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// GET /api/admin/webhooks/event-types
async fn list_event_types(_admin: AdminUser) -> Result<impl IntoResponse, ApiError> {
    let types: Vec<EventTypeInfo> = AuditEventType::all()
        .iter()
        .map(|e| EventTypeInfo {
            value: e.as_str().to_string(),
            label: humanize(e.as_str()),
        })
        .collect();
    Ok(Json(ApiResponse::success(types)))
}

// ---------------------------------------------------------------------------
// Webhook handlers
// ---------------------------------------------------------------------------

/// POST /api/admin/webhooks
async fn create_webhook(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    validate_url(&req.url)?;
    validate_event_types(&req.event_types)?;

    let new_webhook = NewWebhook {
        name: req.name,
        url: req.url,
        enabled: req.enabled.unwrap_or(true),
        signing_secret: generate_signing_secret(),
        created_by: Some(admin.0.id),
    };

    let webhook = state
        .db
        .create_webhook(&new_webhook, &req.event_types, &req.auth_header_ids)?;

    audit(
        &state,
        AuditEventType::WebhookCreated,
        admin.0.id,
        serde_json::json!({ "webhook_id": webhook.id, "name": webhook.name, "url": webhook.url }),
    );

    let resp = to_webhook_response(&state, webhook)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::success(resp))))
}

/// GET /api/admin/webhooks
async fn list_webhooks(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let webhooks = state.db.list_webhooks()?;
    let mut resp = Vec::with_capacity(webhooks.len());
    for wh in webhooks {
        resp.push(to_webhook_response(&state, wh)?);
    }
    Ok(Json(ApiResponse::success(resp)))
}

/// GET /api/admin/webhooks/{id}
async fn get_webhook(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let webhook = state.db.get_webhook(id)?;
    let resp = to_webhook_response(&state, webhook)?;
    Ok(Json(ApiResponse::success(resp)))
}

/// PATCH /api/admin/webhooks/{id}
async fn update_webhook(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateWebhookRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(url) = &req.url {
        validate_url(url)?;
    }
    if let Some(events) = &req.event_types {
        validate_event_types(events)?;
    }

    let changes = UpdateWebhook {
        name: req.name,
        url: req.url,
        enabled: req.enabled,
        updated_at: Some(Utc::now()),
    };

    let webhook = state.db.update_webhook(
        id,
        &changes,
        req.event_types.as_deref(),
        req.auth_header_ids.as_deref(),
    )?;

    audit(
        &state,
        AuditEventType::WebhookUpdated,
        admin.0.id,
        serde_json::json!({ "webhook_id": id }),
    );

    let resp = to_webhook_response(&state, webhook)?;
    Ok(Json(ApiResponse::success(resp)))
}

/// DELETE /api/admin/webhooks/{id}
async fn delete_webhook(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let deleted = state.db.delete_webhook(id)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Webhook not found".to_string()));
    }

    audit(
        &state,
        AuditEventType::WebhookDeleted,
        admin.0.id,
        serde_json::json!({ "webhook_id": id }),
    );

    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Webhook deleted".to_string()),
        error: None,
    }))
}

/// POST /api/admin/webhooks/{id}/test — deliver a synthetic test event now.
async fn test_webhook(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let webhook = state.db.get_webhook(id)?;
    let payload = crate::webhooks::test_payload();

    match state
        .webhook_dispatcher
        .deliver(&webhook, &payload, "webhook_test", None)
        .await
    {
        Ok(()) => Ok(Json(ApiResponse::success_with_message(
            serde_json::json!({ "delivered": true }),
            "Test webhook delivered successfully".to_string(),
        ))),
        Err(e) => Ok(Json(ApiResponse::success_with_message(
            serde_json::json!({ "delivered": false, "error": e }),
            format!("Test webhook delivery failed: {e}"),
        ))),
    }
}

/// GET /api/admin/webhooks/deliveries
async fn list_deliveries(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<DeliveryQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let deliveries: Vec<WebhookDelivery> =
        state
            .db
            .list_webhook_deliveries(q.webhook_id, limit, offset)?;
    Ok(Json(ApiResponse::success(deliveries)))
}

#[cfg(test)]
mod tests {
    use super::validate_url;

    fn rejected(url: &str) -> bool {
        validate_url(url).is_err()
    }

    #[test]
    fn an_ordinary_destination_is_permitted() {
        assert!(validate_url("https://matrix.example.org/_matrix/hook").is_ok());
        assert!(validate_url("http://example.org:8080/hook").is_ok());
    }

    // Deliberately still permitted. A hackspace webhook plausibly points at a
    // Matrix server, a printer or a Home Assistant box on the LAN, and blocking
    // these would break the primary use case to inconvenience somebody who
    // already holds an admin account.
    #[test]
    fn a_private_address_is_still_permitted() {
        assert!(validate_url("http://10.0.0.9:8080/hook").is_ok());
        assert!(validate_url("http://192.168.1.50/hook").is_ok());
        assert!(validate_url("http://172.16.4.4/hook").is_ok());
    }

    #[test]
    fn the_cloud_metadata_address_is_refused() {
        assert!(rejected("http://169.254.169.254/latest/meta-data/"));
        assert!(rejected("https://169.254.169.254/"));
        assert!(rejected("http://169.254.169.254:80/computeMetadata/v1/"));
    }

    // The whole 169.254.0.0/16 block, not just the famous address: the metadata
    // service is not the only thing that answers on link-local, and an
    // allowlist of one address is a denylist wearing a disguise.
    #[test]
    fn the_rest_of_the_link_local_range_is_refused_too() {
        assert!(rejected("http://169.254.0.1/"));
        assert!(rejected("http://169.254.255.254/"));
    }

    // `169.254.169.254` is also `2852039166`, `0xa9fea9fe` and `0251.0376.0251.0376`.
    // The URL parser normalises all of them to the same address, which is why
    // the check inspects the parsed host rather than the string -- a
    // `starts_with` test on the text would miss every one of these.
    #[test]
    fn an_encoded_metadata_address_is_refused() {
        for url in [
            "http://2852039166/",
            "http://0xa9fea9fe/",
            "http://0251.0376.0251.0376/",
        ] {
            assert!(rejected(url), "{url} was permitted");
        }
    }

    #[test]
    fn userinfo_does_not_smuggle_a_host_past_the_check() {
        // `http://example.org@169.254.169.254/` addresses the metadata service,
        // with `example.org` as a username. Reading the host is what tells them
        // apart.
        assert!(rejected("http://example.org@169.254.169.254/"));
    }

    #[test]
    fn ipv6_link_local_and_the_aws_v6_metadata_address_are_refused() {
        assert!(rejected("http://[fe80::1]/"));
        assert!(rejected("http://[fe80::200:5aee:feaa:20a2]/"));
        // AWS publishes IMDS over IPv6 at a unique-local address, so a check
        // that stopped at fe80::/10 would leave the target reachable.
        assert!(rejected("http://[fd00:ec2::254]/"));
    }

    #[test]
    fn another_unique_local_address_is_still_permitted() {
        // fd00::/8 is where self-hosted IPv6 LANs live. Only the one AWS
        // publishes IMDS on is refused.
        assert!(validate_url("http://[fd12:3456::1]/hook").is_ok());
    }

    #[test]
    fn a_non_http_scheme_is_refused() {
        assert!(rejected("file:///etc/passwd"));
        assert!(rejected("javascript:alert(1)"));
        assert!(rejected("ftp://example.org/"));
    }

    #[test]
    fn something_that_is_not_a_url_is_refused_rather_than_parsed_loosely() {
        assert!(rejected(""));
        assert!(rejected("not a url"));
        assert!(rejected("http://"));
    }

    // Stated as a limit rather than left to be discovered: this reads the
    // literal in the URL, and nothing re-checks after DNS. A name whose A
    // record is 169.254.169.254 passes.
    #[test]
    fn a_hostname_is_not_resolved_and_therefore_not_checked() {
        assert!(validate_url("http://metadata.example.org/").is_ok());
    }
}
