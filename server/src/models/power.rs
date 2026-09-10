//! Physical power topology (#41 / #42): circuits -> outlets -> receptacles.
//!
//! A circuit is a breaker with a voltage rating and an amperage limit; it may
//! nest under an upstream/trunk circuit (`parent_circuit_id`, a self-reference
//! walked manually and cycle-guarded at the API layer, like `places`). A circuit
//! carries no `place_id`: it can feed outlets in more than one room, so its room
//! membership emerges from its outlets. An outlet belongs to exactly one room
//! (`place_id`) and one circuit. A receptacle is one socket on an outlet; a tool
//! plugs into zero or one receptacle (`tools.receptacle_id`).

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{power_circuits, power_outlets, power_receptacles, tool_power_state};

// ── Circuits ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = power_circuits)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PowerCircuit {
    pub id: Uuid,
    pub breaker_label: String,
    pub voltage_rating: i32,
    pub amperage_limit: BigDecimal,
    /// Upstream/trunk circuit; `None` for a top-level circuit.
    pub parent_circuit_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Emergency-lockout state (#44). Appended last for the positional
    /// `Queryable`. `locked_out` true = the circuit tripped and is held off
    /// until a staff member clears it; `lockout_source` records which layer
    /// tripped it.
    pub locked_out: bool,
    pub lockout_reason: Option<String>,
    pub lockout_source: Option<String>,
    pub locked_out_at: Option<DateTime<Utc>>,
    pub locked_out_by: Option<Uuid>,
}

#[derive(Debug, Clone, Insertable, Deserialize)]
#[diesel(table_name = power_circuits)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewPowerCircuit {
    pub breaker_label: String,
    pub voltage_rating: i32,
    pub amperage_limit: BigDecimal,
    #[serde(default)]
    pub parent_circuit_id: Option<Uuid>,
}

#[derive(Debug, Clone, AsChangeset, Default, Deserialize)]
#[diesel(table_name = power_circuits)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdatePowerCircuit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaker_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_rating: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amperage_limit: Option<BigDecimal>,
    /// Outer `Option` = "field present?"; inner `Option` = "set to NULL?" so a
    /// circuit can be promoted to a top-level trunk with `Some(None)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_circuit_id: Option<Option<Uuid>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

// ── Outlets ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = power_outlets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PowerOutlet {
    pub id: Uuid,
    pub circuit_id: Uuid,
    pub place_id: Uuid,
    pub label: String,
    /// Free-text position within the room (doors.location precedent).
    pub location: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable, Deserialize)]
#[diesel(table_name = power_outlets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewPowerOutlet {
    pub circuit_id: Uuid,
    pub place_id: Uuid,
    pub label: String,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Clone, AsChangeset, Default, Deserialize)]
#[diesel(table_name = power_outlets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdatePowerOutlet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

// ── Receptacles ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = power_receptacles)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PowerReceptacle {
    pub id: Uuid,
    pub outlet_id: Uuid,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable, Deserialize)]
#[diesel(table_name = power_receptacles)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewPowerReceptacle {
    pub outlet_id: Uuid,
    pub label: String,
}

#[derive(Debug, Clone, AsChangeset, Default, Deserialize)]
#[diesel(table_name = power_receptacles)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdatePowerReceptacle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outlet_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

// ── Tool power state (#43): latest-per-tool telemetry ────────────────────────

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = tool_power_state, primary_key(tool_id))]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolPowerState {
    pub tool_id: Uuid,
    /// Most recent measured draw (amps). `None` until the first report.
    pub last_draw_amps: Option<BigDecimal>,
    pub last_voltage: Option<BigDecimal>,
    /// Firmware-declared limits about the device itself (firmware-owned, not
    /// admin config -- see the migration).
    pub reported_max_voltage: Option<BigDecimal>,
    pub reported_amperage_limit: Option<BigDecimal>,
    pub last_reported_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Tool-scoped emergency lockout (#44): set when this tool's firmware
    /// self-trips on its own over-current limit. Appended last for the
    /// positional `Queryable`. Cleared only by a staff re-enable.
    pub locked_out: bool,
    pub lockout_reason: Option<String>,
    pub locked_out_at: Option<DateTime<Utc>>,
}

/// One reading, inserted-or-updated (`ON CONFLICT (tool_id)`) so the table keeps
/// only the latest per tool.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = tool_power_state)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewToolPowerState {
    pub tool_id: Uuid,
    pub last_draw_amps: Option<BigDecimal>,
    pub last_voltage: Option<BigDecimal>,
    pub reported_max_voltage: Option<BigDecimal>,
    pub reported_amperage_limit: Option<BigDecimal>,
    pub last_reported_at: DateTime<Utc>,
}
