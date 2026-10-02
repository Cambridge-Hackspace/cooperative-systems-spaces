//! Alert classification and acknowledgement (#87).
//!
//! An alert is not a second kind of record: it is an audit event whose type
//! carries a severity worth a person's attention. Every `AuditEventType` has a
//! [`Category`] and a [`Severity`], assigned in Rust by an exhaustive match
//! (`AuditEventType::category` / `::severity` in `models.rs`), so a new event
//! type cannot be added without deciding both. The database holds no copy of
//! this classification: the alert feed asks for `event_type IN (..)` over the
//! types at or above a severity, computed from the enum, and webhook class
//! subscriptions are matched the same way. One source of truth.
//!
//! Acknowledgement lives BESIDE the audit row, never in it: `audit_logs` is
//! append-only, and that is what makes it an audit.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{alert_acknowledgements, alert_heartbeat_runs};

/// What part of the system an event is about. Coarse on purpose: this is a
/// filter an operator reaches for, not a taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    User,
    Training,
    Tool,
    Card,
    Device,
    Webhook,
    Mfa,
    Door,
    Place,
    Module,
    Bypass,
    Power,
    Schedule,
    Mail,
    Membership,
    Cmi5,
    Groupsio,
    Rbac,
    Alerts,
}

impl Category {
    pub const ALL: [Category; 19] = [
        Category::User,
        Category::Training,
        Category::Tool,
        Category::Card,
        Category::Device,
        Category::Webhook,
        Category::Mfa,
        Category::Door,
        Category::Place,
        Category::Module,
        Category::Bypass,
        Category::Power,
        Category::Schedule,
        Category::Mail,
        Category::Membership,
        Category::Cmi5,
        Category::Groupsio,
        Category::Rbac,
        Category::Alerts,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Category::User => "user",
            Category::Training => "training",
            Category::Tool => "tool",
            Category::Card => "card",
            Category::Device => "device",
            Category::Webhook => "webhook",
            Category::Mfa => "mfa",
            Category::Door => "door",
            Category::Place => "place",
            Category::Module => "module",
            Category::Bypass => "bypass",
            Category::Power => "power",
            Category::Schedule => "schedule",
            Category::Mail => "mail",
            Category::Membership => "membership",
            Category::Cmi5 => "cmi5",
            Category::Groupsio => "groupsio",
            Category::Rbac => "rbac",
            Category::Alerts => "alerts",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// How much a person should care. Ordered: `Info < Notice < Warning <
/// Critical`. The alert feed shows `Notice` and above by default; `Info` is
/// the ordinary audit trail (logins, edits, records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// A record. Nothing to act on.
    Info,
    /// Worth seeing: a refusal, a change of who may do what, a lifecycle end.
    Notice,
    /// Something is wrong and will stay wrong until someone looks.
    Warning,
    /// A safety event: power where it should not be, a lockout, a trip.
    Critical,
}

impl Severity {
    pub const ALL: [Severity; 4] = [
        Severity::Info,
        Severity::Notice,
        Severity::Warning,
        Severity::Critical,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Notice => "notice",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }

    /// Stable rank for comparisons stored outside Rust (a webhook subscription's
    /// minimum severity, say).
    pub fn rank(&self) -> i16 {
        match self {
            Severity::Info => 0,
            Severity::Notice => 1,
            Severity::Warning => 2,
            Severity::Critical => 3,
        }
    }

    pub fn parse(s: &str) -> Option<Severity> {
        Severity::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// One acknowledgement of one alert. The primary key is the audit row, so an
/// alert is acknowledged at most once; a second acknowledgement is a no-op.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = alert_acknowledgements)]
#[diesel(primary_key(audit_log_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct AlertAcknowledgement {
    pub audit_log_id: Uuid,
    /// Who acknowledged. `None` once that account is deleted; a merge
    /// re-points it to the survivor.
    pub user_id: Option<Uuid>,
    pub note: Option<String>,
    pub acknowledged_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = alert_acknowledgements)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewAlertAcknowledgement {
    pub audit_log_id: Uuid,
    pub user_id: Option<Uuid>,
    pub note: Option<String>,
}

/// One recorded heartbeat (#87). The ticker emits a new one when the newest
/// is older than the configured interval.
#[derive(Debug, Clone, Queryable, Selectable, Serialize)]
#[diesel(table_name = alert_heartbeat_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct AlertHeartbeatRun {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub alerts_raised: i32,
    pub unacknowledged: i32,
    pub deliveries_failed: i32,
    pub ok: bool,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = alert_heartbeat_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewAlertHeartbeatRun {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub alerts_raised: i32,
    pub unacknowledged: i32,
    pub deliveries_failed: i32,
    pub ok: bool,
    pub error: Option<String>,
}
