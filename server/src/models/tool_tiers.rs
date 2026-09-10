//! Per-member tool rate tiers (#34): named per-tool rate tiers and a
//! per-(member, tool) assignment. The tool's own `usage_*` columns remain the
//! default ("Standard") rate; a tier is a named alternative a member can be
//! assigned to. See [`crate::tool_billing`] for how a tier is resolved into a
//! charge.

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{tool_rate_tiers, tool_tier_assignments};

// ── Rate tiers ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = tool_rate_tiers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolRateTier {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub name: String,
    /// Null behaves like the tool columns (no flat fee / no time charge); a
    /// "Free" tier is `rate_per_min = 0`.
    pub flat_fee: Option<BigDecimal>,
    pub rate_per_min: Option<BigDecimal>,
    pub max_session_minutes: Option<i32>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable, Deserialize)]
#[diesel(table_name = tool_rate_tiers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewToolRateTier {
    pub tool_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub flat_fee: Option<BigDecimal>,
    #[serde(default)]
    pub rate_per_min: Option<BigDecimal>,
    #[serde(default)]
    pub max_session_minutes: Option<i32>,
}

#[derive(Debug, Clone, AsChangeset, Default, Deserialize)]
#[diesel(table_name = tool_rate_tiers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateToolRateTier {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flat_fee: Option<Option<BigDecimal>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_min: Option<Option<BigDecimal>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_session_minutes: Option<Option<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

// ── Assignments ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = tool_tier_assignments)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolTierAssignment {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tool_id: Uuid,
    pub tier_id: Uuid,
    pub assigned_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = tool_tier_assignments)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewToolTierAssignment {
    pub user_id: Uuid,
    pub tool_id: Uuid,
    pub tier_id: Uuid,
    pub assigned_by: Option<Uuid>,
}
