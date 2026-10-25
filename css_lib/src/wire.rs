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
/// `resource/request/scan`: a card presented to a resource (#182).
///
/// **One request for every kind of resource.** A door, a tool and a room are
/// the same thing on this wire: something a badge is presented to, whose
/// bindings decide what follows a yes. `resource` is the resource's UUID or its
/// short external id, resolved the way a tool id always has been. The old
/// `toolguard/request/tool-on { card, tool_id }` and
/// `door/request/scan { door_id, card_id }` are accepted as aliases for one
/// release and then go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalScan {
    pub resource: String,
    pub card: String,
}

/// `resource/request/stop`: end the session on a resource (#182). `card` is
/// optional -- a stop is a safety act and needs no badge; a device that read
/// none names the resource alone. Alias: `toolguard/request/tool-off`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalStop {
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
}

/// `resource/request/usage`: usage to bill or record (#182). The same two
/// durations as [`LocalToolLogRequest`], which is its alias on
/// `toolguard/request/tool-log`; `card` optional for the same reason (#174).
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

/// `resource/request/power`: a power module's reading (#182). The same fields
/// as its alias `toolguard/request/power`, naming the resource. Decimal fields
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
/// can tell its own verdict from a neighbour's. `authorized` exactly `true` is
/// the only yes. `reason` is for humans. `strike` is present only on a yes for
/// a resource with a latch: the pulse the device may perform. This one reply
/// replaces both `toolguard/response/tool-on` and `door/response/unlock`,
/// which the edge keeps publishing alongside it for one release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalScanReply {
    pub resource: String,
    pub authorized: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strike: Option<LocalStrike>,
}

impl LocalScanReply {
    /// The resource-shaped twin of a tool-on reply.
    pub fn from_tool(reply: &LocalToolOnResponse) -> Self {
        LocalScanReply {
            resource: reply.tool_id.clone(),
            authorized: reply.authorized,
            reason: reply.reason.clone(),
            strike: None,
        }
    }

    /// The resource-shaped twin of a door unlock: a yes carries the pulse.
    pub fn from_door(door_id: &str, granted: bool, duration_ms: i32, reason: Option<&str>) -> Self {
        LocalScanReply {
            resource: door_id.to_string(),
            authorized: granted,
            reason: reason.unwrap_or_default().to_string(),
            strike: granted.then_some(LocalStrike {
                pulse_ms: duration_ms,
            }),
        }
    }
}

/// `resource/lease`: one [`ToolLeasePayload`] with the resource named as such
/// (#182). Same meaning, same timing, same rules; the edge publishes both for
/// one release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLeasePayload {
    pub resource: String,
    pub device_id: String,
    pub grant: bool,
    pub ttl_ms: i64,
    pub reason: Option<String>,
}

impl From<&ToolLeasePayload> for ResourceLeasePayload {
    fn from(l: &ToolLeasePayload) -> Self {
        ResourceLeasePayload {
            resource: l.tool_id.clone(),
            device_id: l.device_id.clone(),
            grant: l.grant,
            ttl_ms: l.ttl_ms,
            reason: l.reason.clone(),
        }
    }
}

pub mod local {
    // ── the wire, resource-shaped (#182) ──────────────────────────────────
    //
    // A door, a tool and a room are the same thing here: a resource a badge is
    // presented to. One scan, one stop, one usage report, one power report,
    // one lease -- named by `resource`. The `toolguard/*` and `door/*` topics
    // below are ALIASES for one release: still subscribed, still published,
    // and removed in the next minor.

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
    /// The cached allow-list, pushed whenever it changes.
    pub const RESOURCE_STATE: &str = "resource/state";
    /// One [`super::ResourceLeasePayload`] per message; see [`TOOL_LEASE`] for
    /// the rules, which are unchanged.
    pub const RESOURCE_LEASE: &str = "resource/lease";

    // ── module → edge (the edge subscribes) -- ALIASES, one release ────────

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

    // ── edge → module (the edge publishes) -- ALIASES, one release ─────────

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

    // ── the resource-shaped wire (#182) ───────────────────────────────────

    #[test]
    fn a_scan_reply_from_a_tool_carries_no_strike_and_omits_the_field() {
        let reply = LocalScanReply::from_tool(&LocalToolOnResponse {
            tool_id: "laser-01".into(),
            authorized: true,
            reason: "authorized".into(),
        });
        let v = serde_json::to_value(&reply).unwrap();
        assert_eq!(v["resource"], "laser-01");
        assert_eq!(v["authorized"], true);
        assert!(v.get("strike").is_none(), "a tool reply grew a strike: {v}");
    }

    #[test]
    fn a_scan_reply_from_a_door_carries_the_pulse_only_on_a_yes() {
        let yes = LocalScanReply::from_door("d-1", true, 4000, None);
        assert_eq!(yes.strike, Some(LocalStrike { pulse_ms: 4000 }));
        let no = LocalScanReply::from_door("d-1", false, 0, Some("denied"));
        assert_eq!(no.strike, None, "a refusal carried a pulse");
        assert_eq!(no.reason, "denied");
        assert!(!no.authorized);
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
    fn a_resource_lease_is_the_tool_lease_renamed_and_nothing_else() {
        let l = ToolLeasePayload {
            tool_id: "t".into(),
            device_id: "d".into(),
            grant: true,
            ttl_ms: 3000,
            reason: None,
        };
        let r = ResourceLeasePayload::from(&l);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["resource"], "t");
        assert_eq!(v["device_id"], "d");
        assert_eq!(v["grant"], true);
        assert_eq!(v["ttl_ms"], 3000);
        assert!(v.get("tool_id").is_none());
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
