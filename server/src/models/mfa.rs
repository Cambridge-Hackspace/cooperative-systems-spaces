use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::{user_mfa_recovery_codes, user_mfa_totp, user_mfa_webauthn};

// ---------------------------------------------------------------------------
// TOTP
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = user_mfa_totp)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserMfaTotp {
    pub id: Uuid,
    pub user_id: Uuid,
    pub secret_base32: String,
    /// `None` while setup is in progress; set on first successful verification.
    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_mfa_totp)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserMfaTotp {
    pub user_id: Uuid,
    pub secret_base32: String,
}

// ---------------------------------------------------------------------------
// WebAuthn
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = user_mfa_webauthn)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserMfaWebauthn {
    pub id: Uuid,
    pub user_id: Uuid,
    pub credential_id: Vec<u8>,
    /// Serialized `webauthn_rs::prelude::Passkey`.
    pub passkey: serde_json::Value,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_mfa_webauthn)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserMfaWebauthn {
    pub user_id: Uuid,
    pub credential_id: Vec<u8>,
    pub passkey: serde_json::Value,
    pub label: String,
}

// ---------------------------------------------------------------------------
// Recovery codes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = user_mfa_recovery_codes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserMfaRecoveryCode {
    pub id: Uuid,
    pub user_id: Uuid,
    pub code_hash: String,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_mfa_recovery_codes)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserMfaRecoveryCode {
    pub user_id: Uuid,
    pub code_hash: String,
}
