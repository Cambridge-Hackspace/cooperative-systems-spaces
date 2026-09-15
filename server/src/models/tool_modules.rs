//! Tool module bindings + safety interlocks (#83, data foundation).
//!
//! Vocabularies (role, on_disconnect, interlock kind/condition/reset/enforcement)
//! are TEXT columns guarded by CHECK constraints in the migration; these consts are
//! the Rust-side source of truth. `checks/tests/tool_module_vocab_matches.rs` asserts
//! the two agree, so a value can't be added on one side and forgotten on the other.

use crate::schema::{tool_interlocks, tool_modules};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// A module's role in a tool's access chain.
pub mod module_role {
    pub const READER: &str = "reader";
    pub const POWER: &str = "power";
    pub const SENSOR: &str = "sensor";
    pub const ALL: [&str; 3] = [READER, POWER, SENSOR];
}

/// Fail-safe behaviour when a module loses its link (#83 sec 6).
pub mod on_disconnect {
    pub const FAIL_OFF: &str = "fail_off";
    pub const HOLD_LAST: &str = "hold_last";
    pub const IGNORE: &str = "ignore";
    pub const ALL: [&str; 3] = [FAIL_OFF, HOLD_LAST, IGNORE];
}

/// Interlock kind: an AND-ed start precondition or an OR-ed trip.
pub mod interlock_kind {
    pub const START_GATE: &str = "start_gate";
    pub const TRIP: &str = "trip";
    pub const ALL: [&str; 2] = [START_GATE, TRIP];
}

/// The enumerated condition vocabulary an interlock can reference.
pub mod interlock_condition {
    pub const DOOR_OPEN: &str = "door_open";
    pub const ESTOP: &str = "estop";
    pub const LID_OPEN: &str = "lid_open";
    pub const FLOW_OK: &str = "flow_ok";
    pub const AUTHORIZED: &str = "authorized";
    pub const AUTH_EXPIRED: &str = "auth_expired";
    pub const DRAW_OVER: &str = "draw_over";
    pub const MODULE_OFFLINE: &str = "module_offline";
    pub const ALL: [&str; 8] = [
        DOOR_OPEN,
        ESTOP,
        LID_OPEN,
        FLOW_OK,
        AUTHORIZED,
        AUTH_EXPIRED,
        DRAW_OVER,
        MODULE_OFFLINE,
    ];
}

/// How a latched trip re-arms.
pub mod interlock_reset {
    pub const RE_AUTH: &str = "re_auth";
    pub const OPERATOR_ACK: &str = "operator_ack";
    pub const AUTO: &str = "auto";
    pub const ALL: [&str; 3] = [RE_AUTH, OPERATOR_ACK, AUTO];
}

/// Where an interlock is enforced (#83 sec 5).
pub mod enforcement {
    pub const FIRMWARE: &str = "firmware";
    pub const EDGE: &str = "edge";
    pub const SERVER: &str = "server";
    pub const ALL: [&str; 3] = [FIRMWARE, EDGE, SERVER];
}

/// A binding of one `space_device` to a tool in a role.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = tool_modules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolModule {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub device_id: Uuid,
    pub role: String,
    pub name: String,
    pub params: Value,
    pub on_disconnect: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A new tool module binding for insertion.
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = tool_modules)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewToolModule {
    pub tool_id: Uuid,
    pub device_id: Uuid,
    pub role: String,
    pub name: String,
    pub params: Value,
    pub on_disconnect: String,
}

/// A configurable safety interlock rule for a tool.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = tool_interlocks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolInterlock {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub kind: String,
    pub condition: String,
    pub source_module_id: Option<Uuid>,
    pub debounce_ms: i32,
    pub latch: bool,
    pub reset: String,
    pub enforcement: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A new interlock rule for insertion.
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = tool_interlocks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewToolInterlock {
    pub tool_id: Uuid,
    pub kind: String,
    pub condition: String,
    pub source_module_id: Option<Uuid>,
    pub debounce_ms: i32,
    pub latch: bool,
    pub reset: String,
    pub enforcement: String,
    pub enabled: bool,
}
