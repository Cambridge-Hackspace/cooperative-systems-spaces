//! Single-use, expiring tokens for password reset and email confirmation.
//!
//! Two near-identical types rather than one with a discriminator column. The
//! duplication is deliberate and is explained in full in the migration that
//! creates the tables: a shared table means every consume query has to remember
//! `AND purpose = '...'`, and a query that forgets it accepts a link sent
//! merely to confirm an address as authorization to set a password. Two Diesel
//! types make that mistake unexpressible rather than merely unlikely.
//!
//! Neither struct carries the token. Only its SHA-256 is stored, so nothing
//! that can read this table can use what it reads.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::schema::{email_verification_tokens, password_reset_tokens};

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = password_reset_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PasswordResetToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    /// Set by the claiming UPDATE. `None` means live; `Some` means spent.
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = password_reset_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewPasswordResetToken {
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = email_verification_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct EmailVerificationToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = email_verification_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewEmailVerificationToken {
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}
