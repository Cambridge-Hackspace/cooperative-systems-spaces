use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::{
    webhook_auth_headers, webhook_auth_links, webhook_class_subscriptions, webhook_deliveries,
    webhook_event_subscriptions, webhooks,
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
    /// Delivery envelope (#87): one of [`webhook_format`]. Last field for
    /// the positional-Queryable reason the User model documents.
    pub format: String,
}

/// The delivery envelopes a webhook may ask for. Held to the `format` CHECK
/// in the migration by `checks/tests/alert_severity_vocab_matches.rs`.
pub mod webhook_format {
    /// The audit event as JSON, HMAC-signed. The default.
    pub const JSON: &str = "json";
    /// A Discord incoming-webhook message: `content` plus one embed.
    pub const DISCORD: &str = "discord";
    pub const ALL: [&str; 2] = [JSON, DISCORD];
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
    pub format: String,
}

#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = webhooks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateWebhook {
    pub name: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
    pub format: Option<String>,
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
// webhook_class_subscriptions (#87)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = webhook_class_subscriptions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct WebhookClassSubscription {
    pub id: Uuid,
    pub webhook_id: Uuid,
    /// `None` = every category.
    pub category: Option<String>,
    /// A `Severity::as_str` value; the floor, inclusive.
    pub min_severity: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = webhook_class_subscriptions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewWebhookClassSubscription {
    pub webhook_id: Uuid,
    pub category: Option<String>,
    pub min_severity: String,
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
