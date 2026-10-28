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
    /// The policy for every resource this device coordinates, one shape
    /// whatever the kind (#191): [`super::ResourcePolicyPayload`]. The push
    /// twin of `GET /api/resources/policy`. Not `resource/state`, which is
    /// what the edge publishes on the LOCAL broker: that is the allow-list
    /// mirror for modules, and one name for two payloads on two wires would
    /// be the confusion this convergence exists to end.
    pub const RESOURCE_POLICY: &str = "resource/policy";
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

/// The policy for every resource one device coordinates, in one shape (#191).
///
/// Until this, "who may use this resource" reached the edge in two
/// representations: per-principal resource lists for tools (`toolguard/sync`)
/// and per-resource card lists for doors (`doors/state`), and the edge decided
/// each in its own order. This is both at once, per resource: a door's
/// compiled lists and hold, a tool's status and online rule, and every
/// resource's kind, session profile, pulse and whether it is in use -- with
/// the principals (members, as digests) beside them, which the tool rule and
/// the kiosk still read. `toolguard/sync` and `doors/state` are aliases of it
/// for one release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePolicyPayload {
    /// The device this policy is for; a device holds only what it is bound to.
    pub device_id: String,
    /// When the server built it (RFC 3339). Advisory, as on `module/state`.
    pub as_of: String,
    /// Which profile field the card digests were taken from.
    pub profile_field: String,
    pub resources: Vec<ResourcePolicy>,
    pub principals: Vec<Principal>,
}

/// One resource's policy, whatever its kind (#191).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePolicy {
    /// The resource's UUID.
    pub id: String,
    pub external_id: Option<String>,
    pub name: String,
    /// `tool` | `door`.
    pub kind: String,
    /// `momentary` | `until_stopped`; see `module/state`.
    #[serde(default)]
    pub session_profile: String,
    /// A tool's `idle` | `in_use` | `maintenance` | `broken` | `repair` |
    /// `retired`, as on the allow-list; empty for a door.
    #[serde(default)]
    pub status: String,
    /// A tool the server insists on deciding itself; never set on a door.
    #[serde(default)]
    pub requires_online: bool,
    /// A session is open on it as the server knows it (#190).
    #[serde(default)]
    pub in_use: bool,
    /// A door that is authored but must not open; always true for a tool.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The strike pulse, for a resource with a `strike` binding (a door's site
    /// default when it has none).
    #[serde(default)]
    pub pulse_ms: Option<i32>,
    /// Card digests a door's allow rules compile to. Empty on a tool: a tool
    /// is decided from `principals`.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Card digests a door's deny rules compile to; `deny` beats `allow`.
    #[serde(default)]
    pub deny: Vec<String>,
    /// The Open Access latch (RFC 3339): held released, no card needed, until
    /// then. Compared against the device's own clock, so the window
    /// self-expires.
    #[serde(default)]
    pub hold_until: Option<String>,
}

fn default_true() -> bool {
    true
}

/// A member as a device may know one (#191): a digest, never the card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    /// `argon2id(device_pepper, card)`, hex-encoded; see `profile_field_digest`.
    pub digest: String,
    pub full_name: String,
    pub is_active: bool,
    /// The resources this member may start, by UUID.
    #[serde(default)]
    pub authorized: Vec<String>,
}

/// One resource's wiring: the modules bound to it and the rules that gate it.
///
/// #182: every resource with a binding appears -- a door with a coordinator and
/// a strike as much as a tool with a plug. `kind` says which; the field names
/// (`tool_id`, `tools`) are the wire's history and stay for the alias release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolModuleTool {
    /// The resource's UUID. Named for the wire's history; a door's id goes here
    /// too.
    pub tool_id: String,
    pub external_id: Option<String>,
    /// `tool` | `door` (#182). Absent on a pre-#182 server, which sent only
    /// tools -- hence the default.
    #[serde(default = "kind_tool")]
    pub kind: String,
    /// How a session on this resource ends (#182): `momentary` (over when the
    /// strike pulse is -- the door as we know it) or `until_stopped` (until a
    /// stop -- the tool, and the room). Empty on a server that predates it,
    /// which a consumer reads as the kind's default.
    #[serde(default)]
    pub session_profile: String,
    /// Whether a session is open on this resource as the server knows it
    /// (#190): a tool `in_use`, or a door whose `until_stopped` session a badge
    /// opened and nothing has yet ended -- a stop, the sweep, an administrator.
    /// The coordinator reconciles its own sessions against this the way it
    /// does against the allow-list's tool status; a module does nothing with
    /// it. Absent on a server that predates it.
    #[serde(default)]
    pub in_use: bool,
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
    /// `reader` | `power` | `sensor` | `edge` | `strike`. `edge` is load-bearing,
    /// not an afterthought: it is the role a coordinator holds on a resource, and
    /// `authorize_power_report` requires it to accept a relayed report. `strike`
    /// (#182) is a door's latch as a binding, with `pulse_ms` as its profile.
    pub role: String,
    pub name: String,
    /// Role-specific configuration (gpio pin, receptacle id, sensor input, ...).
    pub params: serde_json::Value,
    /// `fail_off` | `hold_last` | `ignore` -- what this module does when its link
    /// drops. Deny-biased default is `fail_off` for safety-critical power.
    pub on_disconnect: String,
    /// Start sequencing, for a `power` binding (#179). Bindings in a lower
    /// stage must have **confirmed they are energized** -- a power report from
    /// that device with `relay_on: true`, received since the tool was allowed
    /// -- before this one is leased. Any number of bindings per stage, any
    /// number of stages. Default 0, which is "first".
    #[serde(default)]
    pub start_stage: i32,
    /// After the stage before it has confirmed (or, for the first stage, after
    /// the tool is allowed), wait this long before leasing: spin-up time.
    #[serde(default)]
    pub start_delay_ms: i32,
    /// After *any* denial, keep this binding leased this long: run-on time. It
    /// applies to the coordinator's decisions, never to its silence -- an edge
    /// that dies still lapses every lease at its TTL.
    #[serde(default)]
    pub run_on_ms: i32,
    /// For a `strike` binding (#182): how long the latch is released on a yes,
    /// in ms. The value a door firmware receives as `strike.pulse_ms` in its
    /// scan reply. Zero on every other role.
    #[serde(default)]
    pub pulse_ms: i32,
    /// For a `strike` binding (#182): whether the latch is held released for the
    /// whole of an open-access window (`hold_unlock_until` in `doors/state`), or
    /// only pulsed per badge. A latch that cannot be held energized says
    /// `false`, and the door is then never held.
    #[serde(default = "hold_open_access_default")]
    pub hold_open_access: bool,
}

fn kind_tool() -> String {
    "tool".to_string()
}

fn hold_open_access_default() -> bool {
    true
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
/// `resource/request/scan`: a card presented to a resource (#182).
///
/// **One request for every kind of resource.** A door, a tool and a room are
/// the same thing on this wire: something a badge is presented to, whose
/// bindings decide what follows a yes. `resource` is the resource's UUID or its
/// short external id, resolved the way a tool id always has been. The old
/// `toolguard/request/tool-on { card, tool_id }` and
/// `door/request/scan { door_id, card_id }` were its aliases for one release
/// (v0.2) and are gone (#192).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalScan {
    pub resource: String,
    pub card: String,
}

/// `resource/request/stop`: end the session on a resource (#182). `card` is
/// optional -- a stop is a safety act and needs no badge; a device that read
/// none names the resource alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStop {
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
}

/// `resource/request/usage`: usage to bill or record (#182).
///
/// **Two durations, because a machine's working signal gives two honest answers
/// and they are not interchangeable** (#169). A laser's enable line drops on
/// every scan-line return, so a device watching it can report the whole engrave
/// as continuous machine time, or only the intervals the beam was actually on
/// -- figures that differ by 30-50% on the same job.
///
/// - `seconds` is **the usage the device reports as billable**. The module
///   decides what "in use" means for its machine.
/// - `active_seconds` is the narrower figure: time the working signal was
///   actually asserted. Optional, so every existing module stays valid.
///
/// Which one bills is a per-tool platform setting (`tools.usage_basis`), not a
/// firmware choice. The other is still recorded. `seconds` stays optional
/// (absent reads as `0.0`), and reports **accumulate** server-side in one
/// atomic statement, so a module should prefer periodic increments to a single
/// end-of-session total: a reboot then loses one interval, not the session.
///
/// `card` is optional (#174): a plug never learns a card -- the only thing it
/// receives is a lease, which names a device and not a member -- so it reports
/// naming the resource alone, and the server attributes the usage to whoever
/// is signed on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalUsage {
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_seconds: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// `resource/request/power`: a power module's reading (#182), the local twin
/// of `POST /api/toolguard/power-report`, naming the resource. Decimal fields
/// are numbers here and strings on the HTTP relay; `relay_on` absent means
/// "cannot report", which is not `false` -- and since #179 a `true` is what
/// confirms, for start sequencing, that this device actually energized.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalPower {
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_now: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_now: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_voltage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amperage_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_tripped: Option<bool>,
}

/// What a device holding a `strike` binding may do on a yes: release the latch
/// for `pulse_ms` (#182). Carried on [`LocalScanReply`] only when the resource
/// has a strike and the answer is yes; a reader with no latch ignores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStrike {
    pub pulse_ms: i32,
}

/// `resource/response/scan`: the answer to a [`LocalScan`] (#182).
///
/// `resource` echoes the request, so a device on a broker shared with others
/// can tell its own verdict from a neighbour's -- these topics are not
/// namespaced and carry no device id, so without the echo every reply is
/// addressed to everyone (#166). It is an echo of the request, not a decision,
/// so it belongs in the "is this mine" test and never in the "may I" test.
/// `authorized` exactly `true` is the only yes; a missing field is not a yes.
/// `reason` is for humans and logs -- **branch on `authorized`, display
/// `reason`** -- the strings are not part of the contract. `strike` is
/// present only on a yes for a resource with a latch: the pulse the device
/// may perform. This one reply replaced `toolguard/response/tool-on` and
/// `door/response/unlock` (#182); the aliases are gone (#192).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalScanReply {
    pub resource: String,
    pub authorized: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<LocalStrike>,
}

impl LocalScanReply {
    /// The one reply (#192): the resource echoed, the verdict, the reason, and
    /// -- only on a yes, only for a resource with a latch -- the pulse. A
    /// refusal never carries a strike whatever `pulse_ms` says, so firmware
    /// that pulses on the field's presence alone still cannot open a door on
    /// a no.
    pub fn new(resource: &str, authorized: bool, reason: &str, pulse_ms: Option<i32>) -> Self {
        LocalScanReply {
            resource: resource.to_string(),
            authorized,
            reason: reason.to_string(),
            strike: if authorized {
                pulse_ms.map(|pulse_ms| LocalStrike { pulse_ms })
            } else {
                None
            },
        }
    }
}

/// `resource/lease`: permission for one power module to stay energized for a
/// bounded time (#83, #182). One message per module per renewal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLeasePayload {
    /// The resource this lease governs: always its UUID, never an external id.
    pub resource: String,
    /// The power module the lease is addressed to. Filter on it: every module
    /// on the broker sees every lease.
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

pub mod local {
    // ── the wire, resource-shaped (#182) ──────────────────────────────────
    //
    // A door, a tool and a room are the same thing here: a resource a badge is
    // presented to. One scan, one stop, one usage report, one power report,
    // one lease -- named by `resource`. The `toolguard/*` and `door/*` topic
    // families were aliases of these for one release (v0.2) and are gone
    // (#192): a message on an old name is not subscribed to and gets no reply.

    /// A card presented to a resource: [`super::LocalScan`], `{ resource, card }`.
    pub const SCAN_REQUEST: &str = "resource/request/scan";
    /// End the session on a resource: [`super::LocalStop`], `{ resource, card? }`.
    pub const STOP_REQUEST: &str = "resource/request/stop";
    /// Usage to bill or record: [`super::LocalUsage`].
    pub const USAGE_REQUEST: &str = "resource/request/usage";
    /// A power module's reading: [`super::LocalPower`].
    pub const POWER_REPORT_REQUEST: &str = "resource/request/power";
    /// A device saying it is alive: [`super::LocalAliveRequest`], `{ device_id }`.
    pub const RESOURCE_ALIVE_REQUEST: &str = "resource/request/alive";
    /// The answer to [`SCAN_REQUEST`]: [`super::LocalScanReply`].
    pub const SCAN_RESPONSE: &str = "resource/response/scan";
    /// Ack for [`STOP_REQUEST`]: `{ "ok": true }`, receipt and nothing more.
    pub const STOP_RESPONSE: &str = "resource/response/stop";
    /// Ack for [`USAGE_REQUEST`]: `{ "ok": true }`.
    pub const USAGE_RESPONSE: &str = "resource/response/usage";
    /// Ack for [`POWER_REPORT_REQUEST`]: `{ "ok": true }` -- never permission.
    pub const POWER_REPORT_RESPONSE: &str = "resource/response/power";
    /// The cached allow-list, pushed whenever it changes. The local twin of
    /// `GET /api/toolguard/sync`.
    pub const RESOURCE_STATE: &str = "resource/state";
    /// One [`super::ResourceLeasePayload`] per message: permission for one
    /// power module to stay energized for a bounded time. Republished every
    /// renewal interval for as long as the conditions hold, and simply *not*
    /// published once they stop.
    ///
    /// There is no matching request topic, and that asymmetry is the design.
    /// A module does not ask for a lease; the coordinator offers one, and the
    /// offer ceasing is the instruction to stop.
    pub const RESOURCE_LEASE: &str = "resource/lease";

    /// A kiosk asking the edge to re-push its state. No payload.
    pub const KIOSK_REFRESH_REQUEST: &str = "kiosk/refresh";
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── the resource-shaped wire (#182) ───────────────────────────────────

    #[test]
    fn a_scan_reply_for_a_resource_without_a_latch_omits_the_strike() {
        let reply = LocalScanReply::new("laser-01", true, "authorized", None);
        let v = serde_json::to_value(&reply).unwrap();
        assert_eq!(v["resource"], "laser-01");
        assert_eq!(v["authorized"], true);
        assert_eq!(v["reason"], "authorized");
        assert!(v.get("strike").is_none(), "a tool reply grew a strike: {v}");
    }

    #[test]
    fn a_scan_reply_carries_the_pulse_only_on_a_yes() {
        let yes = LocalScanReply::new("d-1", true, "authorized", Some(4000));
        assert_eq!(yes.strike, Some(LocalStrike { pulse_ms: 4000 }));
        // A refusal never carries a pulse, even when the resource has one:
        // firmware that pulses on the field's presence must not open a door
        // on a no.
        let no = LocalScanReply::new("d-1", false, "denied", Some(4000));
        assert_eq!(no.strike, None, "a refusal carried a pulse");
        assert_eq!(no.reason, "denied");
        assert!(!no.authorized);
        let v = serde_json::to_value(&no).unwrap();
        assert!(v.get("strike").is_none(), "{v}");
    }

    #[test]
    fn a_stop_with_no_card_serializes_without_the_key() {
        let v = serde_json::to_value(LocalStop {
            resource: "r".into(),
            card: None,
        })
        .unwrap();
        assert!(v.get("card").is_none(), "{v}");
        let parsed: LocalStop = serde_json::from_str(r#"{"resource":"r"}"#).unwrap();
        assert_eq!(parsed.card, None);
    }

    #[test]
    fn a_resource_lease_names_the_resource_and_carries_a_null_reason_on_a_grant() {
        let r = ResourceLeasePayload {
            resource: "t".into(),
            device_id: "d".into(),
            grant: true,
            ttl_ms: 3000,
            reason: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["resource"], "t");
        assert_eq!(v["device_id"], "d");
        assert_eq!(v["grant"], true);
        assert_eq!(v["ttl_ms"], 3000);
        assert!(
            v["reason"].is_null(),
            "reason is always present, null on a grant: {v}"
        );
        assert!(
            v.get("tool_id").is_none(),
            "the old name is gone from the wire: {v}"
        );
    }

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
