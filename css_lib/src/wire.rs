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
    pub modules: Vec<DeviceBinding>,
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

/// A device bound to a resource in one role (#101). Was `ToolModuleBinding`; the
/// binding generalized to resources when doors and tools became one model, so the
/// wire struct follows the `device_bindings` table and the server model.
///
/// The enclosing snapshot is still tool-scoped (`ToolModuleStatePayload.tools`) --
/// only tools appear in it -- so those types keep their names, and the serialized
/// field names here are unchanged so the edge's deserialization is untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceBinding {
    pub id: String,
    pub device_id: String,
    /// `reader` | `power` | `sensor` | `edge`. `edge` is load-bearing, not an
    /// afterthought: it is the role a coordinator holds on a resource, and
    /// `authorize_power_report` requires it to accept a relayed report.
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

/// The edge's answer to [`local::TOOL_ON_REQUEST`] (#166).
///
/// A type rather than a `json!` literal for one reason: `tool_id` must not be
/// omissible. The edge answers tool-on from three separate places -- a power
/// lockout, the online-synchronous path, and the cached-allow-list path -- and
/// every module on the broker receives all three. Built by hand, a fourth site
/// would eventually ship without the field and the omission would look exactly
/// like a reply meant for somebody else.
///
/// **`tool_id` is what makes the reply addressable.** These topics carry no
/// device id and are not namespaced, so a module cannot otherwise tell its own
/// answer from a neighbour's: two readers on one broker, and a yes at one
/// machine could open a session at another where someone is waiting for a no.
/// A receiver **must** discard a reply whose `tool_id` is not the one it asked
/// about. It is an echo of the request, not a decision, so it is not inside any
/// authorization test.
///
/// This mirrors what the door half of the same resource model (#101) has always
/// done: `door/response/unlock` echoes `door_id`.
///
/// `reason` is for humans and logs. **Branch on `authorized`, display
/// `reason`** -- the strings are not part of the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalToolOnResponse {
    /// The tool the request named, echoed back unchanged.
    pub tool_id: String,
    /// Whether the session may start. Exactly `true` is the only yes; a missing
    /// field is not a yes.
    pub authorized: bool,
    /// Why, for a human. Never branch on this text.
    pub reason: String,
}

/// A module's usage report on [`local::TOOL_LOG_REQUEST`] (#169).
///
/// A type rather than a shared request struct because tool-log is the only one
/// of the three tool topics that carries durations, and the edge used to parse
/// all three into one struct whose field set was their union. That union is
/// what a documentation oracle sees, so no per-topic check could tell whether
/// tool-on had grown a `seconds` or tool-log had lost one. Same reasoning as
/// [`LocalToolOnResponse`]: the shape a module must get right is worth naming.
///
/// **Two durations, because a machine's working signal gives two honest answers
/// and they are not interchangeable.** A laser's enable line drops on every
/// scan-line return, so a device watching it can report the whole engrave as
/// continuous machine time, or only the intervals the beam was actually on --
/// figures that differ by 30-50% on the same job.
///
/// - `seconds` is **the usage the device reports as billable**, unchanged in
///   meaning since before this type existed. `FIRMWARE.md` has always said the
///   module decides what "in use" means, so nothing deployed changes behaviour.
/// - `active_seconds` is the narrower figure: time the working signal was
///   actually asserted. Optional, so every existing module stays valid.
///
/// Which one bills is a per-tool platform setting (`tools.usage_basis`), not a
/// firmware choice. The other is still recorded -- for an unmetered tool it is
/// pure statistics, and for a metered one it is how an administrator tells a
/// dense engrave from a sparse one at the same bench time.
///
/// **`seconds` stays optional, and that is deliberately not a narrowing.** The
/// struct this replaced defaulted it, and the edge treats absence as `0.0`; a
/// required field here would start dropping payloads that are accepted today.
/// Reports **accumulate** server-side in one atomic statement, so a module
/// should prefer periodic increments to a single end-of-session total: a reboot
/// then loses one interval rather than the whole session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalToolLogRequest {
    /// The card that activated the session, not the last card seen, from a
    /// device that read one; the server checks it against the open session.
    ///
    /// **Optional** (#174). A plug never learns a card -- the only thing it
    /// receives is a lease, which names a device and not a member -- so it
    /// reports naming the tool alone, and the server attributes the usage to
    /// whoever is signed on to that tool. The server already knows; the plug
    /// does not have to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// The tool this usage belongs to.
    pub tool_id: String,
    /// Billable usage, as the device defines it. Absent is treated as `0.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f32>,
    /// Time the working signal was asserted, if the device senses one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_seconds: Option<f32>,
    /// Recorded and surfaced, never billed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// A device saying it is alive, on [`local::ALIVE_REQUEST`] (#170).
///
/// **Every device sends this, whatever its role.** It is the liveness contract
/// on the local broker: the edge credits the device's bindings with having been
/// heard from, and forwards it so the server's `last_seen_at` -- what the
/// silence detector (#84) and the admin device list read -- is written for the
/// device too.
///
/// Before this, the only thing that counted as being alive was a power report.
/// That is a claim about a circuit, and the server gates its relay on the
/// `power` binding -- so a reader or a sensor, which has no power to report,
/// could never be credited at the server at all, and at the edge only by
/// sending a power report it had no business sending. With the default
/// `on_disconnect = fail_off`, a reader bound to a tool therefore denied that
/// tool its lease forever, and the member saw ACCESS GRANTED beside a machine
/// that never came on.
///
/// Carries the device id and nothing else, on purpose. "Health" beyond *alive*
/// -- uptime, signal strength, a fault flag -- is a different message with its
/// own shape; bolting fields onto the liveness contract is how a heartbeat
/// grows into something expensive to send every second.
///
/// A power report still counts as being alive, for plugs that predate this.
/// There is no acknowledgement: the lease is the feedback, and an ack per pulse
/// would double the traffic to say nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalAliveRequest {
    /// The device's own id, from registration. The edge resolves it to every
    /// binding the device holds; firmware cannot know a binding id.
    pub device_id: String,
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

    // `api_key?` used to be documented on all three tool topics. Nothing has
    // ever read it here, and the HTTP form has been refused with 401 since #101
    // slice 6 -- so it was a field a firmware author could implement in good
    // faith and have silently ignored. Removed in #169.

    /// A card was presented at a tool: `{ card, tool_id }`.
    pub const TOOL_ON_REQUEST: &str = "toolguard/request/tool-on";
    /// The session ended: `{ card, tool_id }`.
    pub const TOOL_OFF_REQUEST: &str = "toolguard/request/tool-off";
    /// Usage to bill or record: carries [`LocalToolLogRequest`] --
    /// `{ card, tool_id, seconds, active_seconds?, temperature? }`. Two
    /// durations because a working signal gives two honest answers; which one
    /// bills is `tools.usage_basis`, not the module's choice (#169).
    pub const TOOL_LOG_REQUEST: &str = "toolguard/request/tool-log";
    /// A power module's reading: `{ tool_id, draw_now, voltage_now, relay_on?, ... }`.
    /// Decimal fields are strings; `relay_on` absent means "cannot report",
    /// which is not the same as `false`.
    pub const POWER_REQUEST: &str = "toolguard/request/power";
    /// An RFID scan from a door's hardware bridge: `{ door_id, card_id }`.
    pub const DOOR_SCAN_REQUEST: &str = "door/request/scan";
    /// A kiosk asking the edge to re-push its state. No payload.
    pub const KIOSK_REFRESH_REQUEST: &str = "kiosk/refresh";
    /// A device saying it is alive: carries [`super::LocalAliveRequest`] --
    /// `{ device_id }`. Every role sends it; see the type for why (#170).
    pub const ALIVE_REQUEST: &str = "toolguard/request/alive";

    // ── edge → module (the edge publishes) ───────────────────────────────

    /// Answer to [`TOOL_ON_REQUEST`]: one [`super::LocalToolOnResponse`],
    /// `{ tool_id, authorized, reason }`.
    ///
    /// **This is not the server's `ToolGuardResponse`, and there is no
    /// `tool_on` field on this wire** -- that is the HTTP spelling, and this
    /// comment claimed otherwise until #166. The local rule is `authorized`
    /// exactly `true`; absence of it is not permission.
    ///
    /// Discard a reply whose `tool_id` is not the one you asked about: every
    /// module on the broker receives every answer.
    pub const TOOL_ON_RESPONSE: &str = "toolguard/response/tool-on";
    /// Answer to [`TOOL_OFF_REQUEST`].
    pub const TOOL_OFF_RESPONSE: &str = "toolguard/response/tool-off";
    /// Answer to [`TOOL_LOG_REQUEST`].
    pub const TOOL_LOG_RESPONSE: &str = "toolguard/response/tool-log";
    /// Ack for [`POWER_REQUEST`]: `{ "ok": true }`. An ack, not an instruction
    /// -- a power module must never read this as permission to stay energized.
    pub const POWER_RESPONSE: &str = "toolguard/response/power";
    /// The answer to [`DOOR_SCAN_REQUEST`]: an `edge::doors::LocalUnlockResponse`,
    /// `{ door_id, granted, duration_ms, reason? }`.
    ///
    /// **A deny is published here too**, with `granted: false` and
    /// `duration_ms: 0` -- so a message arriving is not an unlock. Branch on
    /// `granted`. This comment listed only `{ door_id, duration_ms }` until
    /// #166, which read as "anything that arrives is an unlock".
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
