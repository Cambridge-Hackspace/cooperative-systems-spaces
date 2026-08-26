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

fn validate_url(url: &str) -> Result<(), ApiError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ApiError::BadRequest(
            "Webhook URL must start with http:// or https://".to_string(),
        ));
    }
    Ok(())
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
