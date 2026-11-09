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
/// How a `module/state` snapshot was scoped for the device that asked (#214).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeScope {
    /// The device holds `edge` bindings, so it was sent exactly the resources
    /// it coordinates; `dropped` is how many resources it was not sent.
    Coordinator { kept: usize, dropped: usize },
    /// The device holds no `edge` binding anywhere, so it was sent everything --
    /// what every device received before #214. A site that has not adopted
    /// coordinator bindings keeps working; one that has narrows per edge.
    Legacy,
}

impl ToolModuleStatePayload {
    /// The snapshot as one edge should see it (#214): only the resources on
    /// which `device_id` holds an `edge` binding -- the binding that already
    /// decides whose relayed power reports are accepted -- or, for a device
    /// holding none, the whole site, as before.
    ///
    /// Why not everything for everyone: every edge leases, and refuses, every
    /// power binding it holds, every tick. With the whole site in every edge,
    /// two edges on one broker would refuse and grant the same module at once,
    /// and even apart each publishes refusals for modules it will never see.
    pub fn scoped_to_edge(mut self, device_id: &str) -> (Self, EdgeScope) {
        let coordinates = |t: &ToolModuleTool| {
            t.modules
                .iter()
                .any(|m| m.role == "edge" && m.device_id == device_id)
        };
        if !self.tools.iter().any(coordinates) {
            return (self, EdgeScope::Legacy);
        }
        let total = self.tools.len();
        self.tools.retain(coordinates);
        let kept = self.tools.len();
        (
            self,
            EdgeScope::Coordinator {
                kept,
                dropped: total - kept,
            },
        )
    }
}

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

/// The edge ↔ module vocabulary on the local broker. Defined once, in
/// `css_wire` (#219), which the firmware depends on too; re-exported here so
/// every `css_lib::wire::...` import in this workspace stays as it was.
pub use css_wire::{
    LocalAliveRequest, LocalCondition, LocalPower, LocalScan, LocalScanReply, LocalStop,
    LocalStrike, LocalUsage, ResourceLeasePayload, local,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── module/state is scoped per edge (#214) ─────────────────────────────

    fn binding(device: &str, role: &str) -> DeviceBinding {
        DeviceBinding {
            id: format!("b-{device}-{role}"),
            device_id: device.into(),
            role: role.into(),
            name: String::new(),
            params: json!({}),
            on_disconnect: "fail_off".into(),
            start_stage: 0,
            start_delay_ms: 0,
            run_on_ms: 0,
            pulse_ms: 0,
            hold_open_access: false,
        }
    }

    fn tool(id: &str, bindings: &[(&str, &str)]) -> ToolModuleTool {
        ToolModuleTool {
            tool_id: id.into(),
            external_id: None,
            kind: "tool".into(),
            session_profile: String::new(),
            in_use: false,
            modules: bindings.iter().map(|(d, r)| binding(d, r)).collect(),
            interlocks: vec![],
            power_fails_safe: false,
        }
    }

    /// Three resources: A coordinated by edge X, B by edge Y, C by nobody.
    fn site() -> ToolModuleStatePayload {
        ToolModuleStatePayload {
            as_of: "2026-11-08T00:00:00Z".into(),
            tools: vec![
                tool("A", &[("edge-x", "edge"), ("plug-a", "power")]),
                tool("B", &[("edge-y", "edge"), ("plug-b", "power")]),
                tool("C", &[("plug-c", "power")]),
            ],
        }
    }

    fn ids(p: &ToolModuleStatePayload) -> Vec<&str> {
        p.tools.iter().map(|t| t.tool_id.as_str()).collect()
    }

    #[test]
    fn an_edge_is_sent_exactly_the_resources_it_coordinates() {
        // The mutant is `retain` keeping everything (or inverted): edge X would
        // then lease plug-b and plug-c every tick alongside edge Y.
        let (x, scope) = site().scoped_to_edge("edge-x");
        assert_eq!(ids(&x), vec!["A"]);
        assert_eq!(
            scope,
            EdgeScope::Coordinator {
                kept: 1,
                dropped: 2
            }
        );
        let (y, _) = site().scoped_to_edge("edge-y");
        assert_eq!(ids(&y), vec!["B"]);
        assert_eq!(
            x.as_of, "2026-11-08T00:00:00Z",
            "scoping keeps the timestamp"
        );
    }

    #[test]
    fn a_device_with_no_edge_binding_anywhere_is_sent_the_whole_site_as_before() {
        // The fallback that keeps a site which never bound a coordinator
        // working. The mutant is "no edge binding -> nothing".
        let (z, scope) = site().scoped_to_edge("edge-z");
        assert_eq!(ids(&z), vec!["A", "B", "C"]);
        assert_eq!(scope, EdgeScope::Legacy);
    }

    #[test]
    fn only_the_edge_role_makes_a_device_a_coordinator() {
        // plug-a is bound on A, as power. That is not coordinating A; it must
        // get the legacy whole site, not "A because I am bound there". The
        // mutant is matching on device_id alone.
        let (p, scope) = site().scoped_to_edge("plug-a");
        assert_eq!(scope, EdgeScope::Legacy);
        assert_eq!(ids(&p), vec!["A", "B", "C"]);
    }

    // ── a condition report (#213) ──────────────────────────────────────────

    #[test]
    fn a_condition_report_is_exactly_the_four_fields_the_firmware_crate_sends() {
        // The CS:S half of the cross-repo contract: spaces-device pins the same
        // literal for its ConditionReport. A rename on either side fails the
        // other.
        let r = LocalCondition {
            device_id: "dev-1".into(),
            resource: "laser-01".into(),
            condition: "door_open".into(),
            asserted: true,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({ "device_id": "dev-1", "resource": "laser-01", "condition": "door_open", "asserted": true })
        );
        assert_eq!(local::CONDITION_REQUEST, "resource/request/condition");
        // And a report missing `asserted` does not parse as "false".
        assert!(
            serde_json::from_str::<LocalCondition>(
                r#"{"device_id":"d","resource":"r","condition":"door_open"}"#
            )
            .is_err()
        );
    }

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
