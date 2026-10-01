//! A user's email addresses (#118).
//!
//! One row per address; the row flagged `is_primary` is mirrored onto
//! `users.email` / `users.email_verified_at` by a database trigger, in that
//! direction only. The application changes an address HERE and reads the
//! primary from the mirror; a direct write to `users.email` is refused by the
//! `users_email_is_a_mirror` guard. See the migration for why.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use crate::schema::user_emails;

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = user_emails)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserEmail {
    pub id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub is_primary: bool,
    /// When the owner confirmed this address. `None` means unconfirmed.
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = user_emails)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUserEmail {
    pub user_id: Uuid,
    pub email: String,
    pub is_primary: bool,
    pub verified_at: Option<DateTime<Utc>>,
}
