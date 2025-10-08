use crate::schema::{users, sql_types};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User role enum for granular permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow)]
#[diesel(sql_type = sql_types::UserRole)]
pub enum UserRole {
    /// User status unknown (default for security)
    Unknown,
    /// New user, basic access only
    Newbie,
    /// Full member with standard access
    Member,
    /// Staff member with elevated privileges
    Staff,
    /// Administrator with full access
    Admin,
}

// Implement Diesel traits for UserRole enum
impl diesel::serialize::ToSql<sql_types::UserRole, diesel::pg::Pg> for UserRole {
    fn to_sql<'b>(&'b self, out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>) -> diesel::serialize::Result {
        match self {
            UserRole::Unknown => out.write_all(b"unknown")?,
            UserRole::Newbie => out.write_all(b"newbie")?,
            UserRole::Member => out.write_all(b"member")?,
            UserRole::Staff => out.write_all(b"staff")?,
            UserRole::Admin => out.write_all(b"admin")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::UserRole, diesel::pg::Pg> for UserRole {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"unknown" => Ok(UserRole::Unknown),
            b"newbie" => Ok(UserRole::Newbie),
            b"member" => Ok(UserRole::Member),
            b"staff" => Ok(UserRole::Staff),
            b"admin" => Ok(UserRole::Admin),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

impl Default for UserRole {
    fn default() -> Self {
        UserRole::Unknown
    }
}

impl UserRole {
    /// Check if user has admin privileges
    pub fn can_access_admin(&self) -> bool {
        matches!(self, UserRole::Admin)
    }

    /// Check if user has staff privileges
    pub fn can_access_staff(&self) -> bool {
        matches!(self, UserRole::Staff | UserRole::Admin)
    }

    /// Check if user has member privileges
    pub fn can_access_member(&self) -> bool {
        matches!(self, UserRole::Member | UserRole::Staff | UserRole::Admin)
    }

    pub fn is_active_user(&self) -> bool {
        !matches!(self, UserRole::Unknown)
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub full_name: String,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub role: UserRole,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUser {
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub full_name: String,
    pub is_active: Option<bool>,
    pub role: Option<UserRole>,
}

#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateUser {
    pub username: Option<String>,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub full_name: Option<String>,
    pub is_active: Option<bool>,
    pub role: Option<UserRole>,
    pub updated_at: Option<NaiveDateTime>,
}

impl NewUser {
    pub fn new(username: String, email: String, password_hash: String, full_name: String) -> Self {
        Self {
            username,
            email,
            password_hash,
            full_name,
            is_active: Some(true),
            role: Some(UserRole::Newbie),
        }
    }

    pub fn with_role(username: String, email: String, password_hash: String, full_name: String, role: UserRole) -> Self {
        Self {
            username,
            email,
            password_hash,
            full_name,
            is_active: Some(true),
            role: Some(role),
        }
    }
}
