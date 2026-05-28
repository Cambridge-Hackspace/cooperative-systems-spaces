use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::{
    webhook_auth_headers, webhook_auth_links, webhook_deliveries, webhook_event_subscriptions,
    webhooks,
};

// ---------------------------------------------------------------------------
// webhook_auth_headers
// ---------------------------------------------------------------------------

/// A reusable, write-only auth credential (e.g. an `Authorization` header).
///
/// `header_value` is a secret: it is loaded only by the dispatcher when sending
/// a request and is never serialized back to API clients.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = webhook_auth_headers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct WebhookAuthHeader {
    pub id: Uuid,
    pub name: String,
    pub header_name: String,
    pub header_value: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = webhook_auth_headers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewWebhookAuthHeader {
    pub name: String,
    pub header_name: String,
    pub header_value: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = webhook_auth_headers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateWebhookAuthHeader {
    pub name: Option<String>,
    pub header_name: Option<String>,
    pub header_value: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// webhooks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = webhooks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Webhook {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub signing_secret: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = webhooks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewWebhook {
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub signing_secret: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = webhooks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateWebhook {
    pub name: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// webhook_event_subscriptions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = webhook_event_subscriptions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct WebhookEventSubscription {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = webhook_event_subscriptions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewWebhookEventSubscription {
    pub webhook_id: Uuid,
    pub event_type: String,
}

// ---------------------------------------------------------------------------
// webhook_auth_links
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = webhook_auth_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct WebhookAuthLink {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub auth_header_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = webhook_auth_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewWebhookAuthLink {
    pub webhook_id: Uuid,
    pub auth_header_id: Uuid,
}

// ---------------------------------------------------------------------------
// webhook_deliveries
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = webhook_deliveries)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub audit_log_id: Option<Uuid>,
    pub event_type: String,
    pub attempt: i32,
    pub success: bool,
    pub status_code: Option<i32>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub request_payload: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = webhook_deliveries)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewWebhookDelivery {
    pub webhook_id: Uuid,
    pub audit_log_id: Option<Uuid>,
    pub event_type: String,
    pub attempt: i32,
    pub success: bool,
    pub status_code: Option<i32>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub request_payload: Option<serde_json::Value>,
}
