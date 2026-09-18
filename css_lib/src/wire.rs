//! Shared wire envelope for the bidirectional edge ↔ server channel.
//!
//! Both the MQTT transport and the WebSocket transport carry the same JSON
//! bodies; the only difference is whether the body is sent as the topic
//! payload (MQTT) or as the `payload` field of this envelope (WebSocket).
//! The `kind` string is identical to the MQTT topic suffix used today so we
//! can route by it uniformly on both transports.

use serde::{Deserialize, Serialize};

/// A single edge ↔ server message in JSON form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessage {
    /// The topic-suffix-equivalent name; see [`kinds`] for the canonical
    /// string literals used by both sides.
    pub kind: String,
    /// Body. Each `kind` defines its own JSON shape — see the existing MQTT
    /// publish helpers on the server and the matching dispatch on the edge.
    pub payload: serde_json::Value,
}

impl WireMessage {
    pub fn new(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
        }
    }
}

/// Canonical `kind` strings. Use these constants on both sides so the wire
/// vocabulary never drifts because of a typo.
pub mod kinds {
    // ── device → server ──────────────────────────────────────────────────
    pub const HEARTBEAT: &str = "heartbeat";
    pub const DATA: &str = "data";
    pub const DOORS_EVENT: &str = "doors/event";

    // ── server → device ──────────────────────────────────────────────────
    pub const NAME: &str = "name";
    pub const TOOLGUARD_STATE: &str = "toolguard/state";
    pub const DOORS_STATE: &str = "doors/state";
    pub const DOORS_UNLOCK: &str = "doors/unlock";
    /// Power lockout + topology snapshot (#48). Carries the currently-locked
    /// tool ids (the edge caches these and stays fail-secure when disconnected)
    /// and the circuit topology + amperage limits the edge needs to aggregate
    /// draw locally and fast-trip.
    pub const POWER_STATE: &str = "power/state";
    /// Tool module bindings + interlock rules (#83). Carries, per tool, which
    /// devices fill its reader/power/sensor roles and the configured start-gates
    /// and trips. Sent as its own snapshot rather than folded into
    /// `toolguard/state`: that payload is duplicated across five crates (see
    /// `checks/tests/toolguard_wire_types.rs`), and module wiring changes on a
    /// different cadence than the authorization list.
    pub const MODULE_STATE: &str = "module/state";
}

/// The `module/state` snapshot the server pushes to the edge (#83).
///
/// Ids are stringified so this shared type needs no uuid dependency, matching
/// [`PowerStatePayload`]. The edge matches tool ids the same way it matches the
/// allow-list (external_id, then UUID string).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolModuleStatePayload {
    /// When the server built this snapshot (RFC 3339). Advisory only.
    pub as_of: String,
    /// Every tool that has at least one module bound or one interlock defined.
    pub tools: Vec<ToolModuleTool>,
}

/// One tool's wiring: the modules bound to it and the rules that gate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolModuleTool {
    pub tool_id: String,
    pub external_id: Option<String>,
    pub modules: Vec<ToolModuleBinding>,
    pub interlocks: Vec<ToolInterlockRule>,
    /// Whether every power module bound to this tool reaches a safe state on its
    /// own when it stops hearing from the coordinator (#83).
    ///
    /// False is not an error: a tool switched by an unmodifiable plug that holds
    /// its last relay state genuinely cannot, and the coordinator's cut is then a
    /// mitigation rather than an interlock. It is carried here so that gap is
    /// visible to the edge and the admin screen instead of being assumed away.
    #[serde(default)]
    pub power_fails_safe: bool,
}

/// A device filling one role in a tool's access chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolModuleBinding {
    pub id: String,
    pub device_id: String,
    /// `reader` | `power` | `sensor`.
    pub role: String,
    pub name: String,
    /// Role-specific configuration (gpio pin, receptacle id, sensor input, ...).
    pub params: serde_json::Value,
    /// `fail_off` | `hold_last` | `ignore` -- what this module does when its link
    /// drops. Deny-biased default is `fail_off` for safety-critical power.
    pub on_disconnect: String,
}

/// One configured interlock: an AND-ed start precondition or an OR-ed trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInterlockRule {
    pub id: String,
    /// `start_gate` | `trip`.
    pub kind: String,
    /// One of the enumerated conditions (`door_open`, `estop`, ...).
    pub condition: String,
    /// The sensor module supplying the condition, when it comes from one.
    pub source_module_id: Option<String>,
    pub debounce_ms: i32,
    /// A tripped cut stays off until reset. Defaults true server-side.
    pub latch: bool,
    /// `re_auth` | `operator_ack` | `auto`.
    pub reset: String,
    /// `firmware` | `edge` | `server` -- where the trip actually executes.
    pub enforcement: String,
}

/// The `power/state` snapshot the server pushes to the edge (#48). All ids are
/// stringified (UUIDs) and `amperage_limit` is a decimal string, so this shared
/// type needs no uuid/decimal/time dependency; the edge parses as needed and
/// matches tool ids the same way it matches the allow-list (external_id, then
/// UUID string).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PowerStatePayload {
    /// When the server built this snapshot (RFC 3339). Advisory only; lockout is
    /// sticky and never expires on age (deny-biased fail-secure).
    pub as_of: String,
    /// Tools currently locked out (own firmware self-trip OR their circuit).
    pub locked_tool_ids: Vec<String>,
    /// Every circuit and its amperage limit, for edge-local aggregation.
    pub circuits: Vec<PowerStateCircuit>,
    /// Every tool's external id and resolved circuit, for edge-local aggregation.
    pub tools: Vec<PowerStateTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerStateCircuit {
    pub id: String,
    /// Decimal string (amps).
    pub amperage_limit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerStateTool {
    pub id: String,
    pub external_id: Option<String>,
    /// The circuit this tool draws from, or `None` if unmapped.
    pub circuit_id: Option<String>,
}

/// One lease decision for one power module (#83).
///
/// This is the coordinator's half of the mechanism [`local::TOOL_LEASE`]
/// carries. It is produced by the edge's watchdog and consumed by module
/// firmware, so it lives here rather than in either of them.
///
/// `grant: false` is not "do nothing" -- it is an instruction to drop the lease,
/// which a module treats as de-energize. A module that receives nothing at all
/// must reach the same state on its own once its lease runs out; that
/// redundancy is the point, and it is why this type carries no "stop" variant.
///
/// There is deliberately **no issue timestamp**. A module that computed
/// `issued_at + ttl_ms` against its own clock would hold a lease too long
/// whenever the two clocks disagreed, and clock skew between a coordinator and
/// a microcontroller with no RTC is the normal case rather than the exception.
/// The TTL is measured from the moment the message arrives, by the receiver,
/// against its own monotonic timer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolLeasePayload {
    /// The tool this lease governs.
    pub tool_id: String,
    /// The power module the lease is addressed to.
    pub device_id: String,
    /// Whether the module may be energized for the next `ttl_ms`.
    pub grant: bool,
    /// How long the grant is good for, from receipt. The module de-energizes on
    /// its own if it is not renewed within this window.
    pub ttl_ms: i64,
    /// Why the lease was refused. For logs and humans only -- never branch on
    /// it. The vocabulary is not part of the contract and will change.
    pub reason: Option<String>,
}

/// The **edge ↔ module** vocabulary, on the *local* broker.
///
/// [`kinds`] above is the server ↔ device wire: namespaced under
/// `{namespace}/devices/{device_id}/`, spoken over the internet or the site
/// link. This module is the other wire entirely -- unnamespaced topics on the
/// broker the edge runs, spoken by the ToolGuards, card readers and plugs in
/// the same building. A module's counterparty is the edge, not the server, so
/// for firmware this is the vocabulary that matters most.
///
/// These constants lived as string literals in four crates -- `edge`,
/// `toolguard-test-ui`, `toolguard-status-ui` and `kiosk` -- each with its own
/// copy and no way to notice when another changed. That is not a hypothetical:
/// it is how `toolguard-test-ui` came to be speaking the protocol as it stood
/// before #83 and #84, with nothing to say so. One vocabulary, one place, and
/// `checks/tests/the_firmware_protocol_is_documented.rs` holds `FIRMWARE.md` to
/// it in both directions.
pub mod local {
    // ── module → edge (the edge subscribes) ──────────────────────────────

    /// A card was presented at a tool: `{ card, tool_id, api_key? }`.
    pub const TOOL_ON_REQUEST: &str = "toolguard/request/tool-on";
    /// The session ended: `{ card, tool_id, api_key? }`.
    pub const TOOL_OFF_REQUEST: &str = "toolguard/request/tool-off";
    /// Usage to bill or record: `{ card, tool_id, seconds, temperature?, api_key? }`.
    pub const TOOL_LOG_REQUEST: &str = "toolguard/request/tool-log";
    /// A power module's reading: `{ tool_id, draw_now, voltage_now, relay_on?, ... }`.
    /// Decimal fields are strings; `relay_on` absent means "cannot report",
    /// which is not the same as `false`.
    pub const POWER_REQUEST: &str = "toolguard/request/power";
    /// An RFID scan from a door's hardware bridge: `{ door_id, card_id }`.
    pub const DOOR_SCAN_REQUEST: &str = "door/request/scan";
    /// A kiosk asking the edge to re-push its state. No payload.
    pub const KIOSK_REFRESH_REQUEST: &str = "kiosk/refresh";

    // ── edge → module (the edge publishes) ───────────────────────────────

    /// Answer to [`TOOL_ON_REQUEST`]. Carries the server's `ToolGuardResponse`,
    /// so the `tool_on: true`-or-nothing rule applies here exactly as it does
    /// over HTTP: absence of `tool_on` is not permission.
    pub const TOOL_ON_RESPONSE: &str = "toolguard/response/tool-on";
    /// Answer to [`TOOL_OFF_REQUEST`].
    pub const TOOL_OFF_RESPONSE: &str = "toolguard/response/tool-off";
    /// Answer to [`TOOL_LOG_REQUEST`].
    pub const TOOL_LOG_RESPONSE: &str = "toolguard/response/tool-log";
    /// Ack for [`POWER_REQUEST`]: `{ "ok": true }`. An ack, not an instruction
    /// -- a power module must never read this as permission to stay energized.
    pub const POWER_RESPONSE: &str = "toolguard/response/power";
    /// A momentary unlock for a door's relay: `{ door_id, duration_ms }`.
    pub const DOOR_UNLOCK_RESPONSE: &str = "door/response/unlock";
    /// The cached allow-list, pushed whenever it changes. The local twin of
    /// `GET /api/toolguard/sync`.
    pub const STATE: &str = "toolguard/state";
    /// One [`super::ToolLeasePayload`] per message: permission for one power
    /// module to stay energized for a bounded time. Republished every renewal
    /// interval for as long as the conditions hold, and simply *not* published
    /// once they stop.
    ///
    /// There is no matching request topic, and that asymmetry is the design.
    /// A module does not ask for a lease; the coordinator offers one, and the
    /// offer ceasing is the instruction to stop.
    pub const TOOL_LEASE: &str = "toolguard/lease";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip() {
        let m = WireMessage::new(
            kinds::DOORS_STATE,
            json!({ "doors": [], "snapshot_at": "2026-05-28T00:00:00Z" }),
        );
        let bytes = serde_json::to_vec(&m).unwrap();
        let back: WireMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.kind, kinds::DOORS_STATE);
        assert_eq!(back.payload["doors"], json!([]));
    }

    #[test]
    fn empty_payload() {
        let m = WireMessage::new(kinds::HEARTBEAT, serde_json::Value::Null);
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"heartbeat\""));
    }
}
