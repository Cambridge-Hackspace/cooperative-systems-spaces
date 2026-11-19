//! Rows for CS:S as an OpenID Connect provider (#225).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::{
    oauth_authorization_codes, oauth_authorization_requests, oauth_clients, oauth_consents,
    oauth_refresh_tokens, oauth_signing_keys,
};

/// A registered app. `secret_hash` is never serialized: the secret is shown
/// once, when it is made, and only its argon2 hash is kept.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = oauth_clients)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OauthClient {
    pub id: Uuid,
    pub client_id: String,
    pub name: String,
    #[serde(skip)]
    pub secret_hash: Option<String>,
    pub confidential: bool,
    pub redirect_uris: Vec<String>,
    pub post_logout_redirect_uris: Vec<String>,
    pub allowed_scopes: Vec<String>,
    pub skip_consent: bool,
    pub disabled: bool,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = oauth_clients)]
pub struct NewOauthClient {
    pub client_id: String,
    pub name: String,
    pub secret_hash: Option<String>,
    pub confidential: bool,
    pub redirect_uris: Vec<String>,
    pub post_logout_redirect_uris: Vec<String>,
    pub allowed_scopes: Vec<String>,
    pub skip_consent: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = oauth_signing_keys)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OauthSigningKey {
    pub kid: String,
    pub private_key_sealed: String,
    pub public_jwk: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub retired_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = oauth_signing_keys)]
pub struct NewOauthSigningKey {
    pub kid: String,
    pub private_key_sealed: String,
    pub public_jwk: serde_json::Value,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = oauth_authorization_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OauthAuthorizationRequest {
    pub id: Uuid,
    pub client_id: Uuid,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: String,
    pub prompt: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = oauth_authorization_requests)]
pub struct NewOauthAuthorizationRequest {
    pub client_id: Uuid,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: String,
    pub prompt: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable)]
#[diesel(table_name = oauth_authorization_codes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OauthAuthorizationCode {
    pub code_hash: String,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub redirect_uri: String,
    pub scope: String,
    pub nonce: Option<String>,
    pub code_challenge: String,
    pub auth_time: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = oauth_refresh_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OauthRefreshToken {
    pub id: Uuid,
    pub token_hash: String,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub scope: String,
    pub auth_time: DateTime<Utc>,
    pub code_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = oauth_refresh_tokens)]
pub struct NewOauthRefreshToken {
    pub token_hash: String,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub scope: String,
    pub auth_time: DateTime<Utc>,
    pub code_hash: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable)]
#[diesel(table_name = oauth_consents)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct OauthConsent {
    pub user_id: Uuid,
    pub client_id: Uuid,
    pub scopes: Vec<String>,
    pub granted_at: DateTime<Utc>,
}

// ── card sign-in (#241) ────────────────────────────────────────────────────

use crate::schema::card_sign_ins;

/// One card sign-in, from tap to redemption: a ticket while a TOTP code is
/// awaited, then a code for the app. Only hashes are kept.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = card_sign_ins)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct CardSignIn {
    pub id: Uuid,
    pub resource_id: Uuid,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub ticket_hash: Option<String>,
    pub code_hash: Option<String>,
    /// `hwk`, or `hwk otp`: the ID token's amr, space-separated.
    pub amr: String,
    pub tries: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = card_sign_ins)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewCardSignIn {
    pub resource_id: Uuid,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub ticket_hash: Option<String>,
    pub code_hash: Option<String>,
    pub amr: String,
    pub expires_at: DateTime<Utc>,
}

// ── the notification relay (#226) ──────────────────────────────────────────

use crate::schema::{notification_kinds, notification_messages, notification_preferences};

#[derive(Debug, Clone, Queryable, Selectable, Insertable, Serialize)]
#[diesel(table_name = notification_kinds)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NotificationKind {
    pub client_id: Uuid,
    pub kind: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable, Insertable)]
#[diesel(table_name = notification_preferences)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NotificationPreference {
    pub user_id: Uuid,
    pub client_id: Uuid,
    pub kind: String,
    pub email: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = notification_messages)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NotificationMessage {
    pub id: Uuid,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub urgency: String,
    pub dedupe_key: Option<String>,
    pub outcome: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = notification_messages)]
pub struct NewNotificationMessage {
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub urgency: String,
    pub dedupe_key: Option<String>,
    pub outcome: String,
}
