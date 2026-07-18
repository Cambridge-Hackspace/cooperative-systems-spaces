use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::home_links;

/// Audience gate for a home-page link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeLinkAudience {
    /// Visible to everyone — signed-in or not.
    Everyone,
    /// Visible only to signed-out visitors.
    Anonymous,
    /// Any authenticated user (Newbie or higher).
    LoggedIn,
    /// Member or higher.
    Member,
    /// Staff or higher.
    Staff,
}

impl HomeLinkAudience {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Anonymous => "anonymous",
            Self::LoggedIn => "logged_in",
            Self::Member => "member",
            Self::Staff => "staff",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "everyone" => Some(Self::Everyone),
            "anonymous" => Some(Self::Anonymous),
            "logged_in" => Some(Self::LoggedIn),
            "member" => Some(Self::Member),
            "staff" => Some(Self::Staff),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = home_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct HomeLink {
    pub id: Uuid,
    pub label: String,
    pub url: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    /// Stored as text; mapped via `HomeLinkAudience::parse`.
    pub audience: String,
    pub sort_order: i32,
    pub enabled: bool,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Optional expiry. When set and `now() >= expires_at`, the public
    /// endpoint hides the link (admin still sees it).
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = home_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewHomeLink {
    pub label: String,
    pub url: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub audience: String,
    pub sort_order: i32,
    pub enabled: bool,
    pub created_by: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, AsChangeset, Default)]
#[diesel(table_name = home_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateHomeLink {
    pub label: Option<String>,
    pub url: Option<String>,
    pub description: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub audience: Option<String>,
    pub sort_order: Option<i32>,
    pub enabled: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
    /// Outer `Some` = field present in request; inner `None` = clear the expiry.
    pub expires_at: Option<Option<DateTime<Utc>>>,
}
