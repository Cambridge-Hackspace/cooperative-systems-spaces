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
    /// The last TOTP time-step consumed on THIS authenticator (#120/#12). A
    /// code whose step is `<=` this is a replay and is refused. `None` until
    /// the first code is spent. Last fields for the positional-Queryable
    /// reason the User model documents.
    pub last_used_step: Option<i64>,
    /// What the member calls this authenticator (#118: several per user). A
    /// row with `confirmed_at == None` is a setup in progress; the former
    /// `pending_secret_base32` column is gone because a pending setup is now
    /// simply its own unconfirmed row.
    pub label: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_mfa_totp)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserMfaTotp {
    pub user_id: Uuid,
    pub secret_base32: String,
    pub label: String,
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
