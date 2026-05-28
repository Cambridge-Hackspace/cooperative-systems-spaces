use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{door_access_events, door_access_rules, door_checkins, doors};

// ---------------------------------------------------------------------------
// doors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = doors)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Door {
    pub id: Uuid,
    pub name: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub edge_device_id: Option<Uuid>,
    pub unlock_duration_ms: i32,
    pub enabled: bool,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = doors)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewDoor {
    pub name: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub edge_device_id: Option<Uuid>,
    pub unlock_duration_ms: i32,
    pub enabled: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = doors)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateDoor {
    pub name: Option<String>,
    pub location: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub edge_device_id: Option<Option<Uuid>>,
    pub unlock_duration_ms: Option<i32>,
    pub enabled: Option<bool>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// door_access_rules
// ---------------------------------------------------------------------------

/// One of the legal values of `door_access_rules.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DoorRuleKind {
    Role,
    User,
    Card,
}

impl DoorRuleKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::User => "user",
            Self::Card => "card",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "role" => Some(Self::Role),
            "user" => Some(Self::User),
            "card" => Some(Self::Card),
            _ => None,
        }
    }
}

/// One of the legal values of `door_access_rules.effect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DoorRuleEffect {
    Allow,
    Deny,
}

impl DoorRuleEffect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = door_access_rules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DoorAccessRule {
    pub id: Uuid,
    pub door_id: Uuid,
    pub kind: String,
    pub value: String,
    pub effect: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = door_access_rules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewDoorAccessRule {
    pub door_id: Uuid,
    pub kind: String,
    pub value: String,
    pub effect: String,
}

// ---------------------------------------------------------------------------
// door_access_events
// ---------------------------------------------------------------------------

/// Source / nature of an unlock attempt. Stored as text so we can grow the
/// vocabulary without a migration; the CHECK constraint on the column keeps
/// us honest about which values are legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoorAccessMethod {
    Rfid,
    QrCheckin,
    AdminRemote,
}

impl DoorAccessMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rfid => "rfid",
            Self::QrCheckin => "qr_checkin",
            Self::AdminRemote => "admin_remote",
        }
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = door_access_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DoorAccessEvent {
    pub id: Uuid,
    pub door_id: Uuid,
    pub user_id: Option<Uuid>,
    pub method: String,
    pub card_id_attempted: Option<String>,
    pub granted: bool,
    pub reason: Option<String>,
    pub ip_address: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = door_access_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewDoorAccessEvent {
    pub door_id: Uuid,
    pub user_id: Option<Uuid>,
    pub method: String,
    pub card_id_attempted: Option<String>,
    pub granted: bool,
    pub reason: Option<String>,
    pub ip_address: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// door_checkins
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = door_checkins)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct DoorCheckin {
    pub id: Uuid,
    pub door_id: Uuid,
    pub user_id: Uuid,
    pub door_access_event_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = door_checkins)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewDoorCheckin {
    pub door_id: Uuid,
    pub user_id: Uuid,
    pub door_access_event_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}
