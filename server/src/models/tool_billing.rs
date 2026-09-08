//! Data models for metered tool-use billing (Phase 2).
//!
//! A `tool_usage_sessions` row is three things at once: the prepaid **hold**
//! record (its `hold_amount` is subtracted from a member's ledger balance while
//! `status = 'open'`, giving *available* balance for the gate), the typed
//! **usage store** (`reported_seconds`), and the **idempotency anchor** for
//! settling a charge exactly once (`charged_amount` + `ledger_entry_id`, set when
//! the session closes and a single `tool_usage` ledger debit is posted).

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A metered tool-use session.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_usage_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolUsageSession {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub user_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Reserved against available balance while open (prepaid); 0 for postpaid.
    pub hold_amount: BigDecimal,
    /// Accumulated device-reported usage, in seconds. `None` until first report.
    pub reported_seconds: Option<BigDecimal>,
    /// The settled charge; `None` while open.
    pub charged_amount: Option<BigDecimal>,
    /// `open` | `settled` | `abandoned`.
    pub status: String,
    /// The `tool_usage` ledger debit posted at settle; `None` while open.
    pub ledger_entry_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// A session to open (activation). Everything else defaults: `ended_at`,
/// `reported_seconds`, `charged_amount`, `ledger_entry_id` NULL; `status`
/// defaults to `'open'` in the database.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::tool_usage_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewToolUsageSession {
    pub tool_id: Uuid,
    pub user_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub hold_amount: BigDecimal,
}
