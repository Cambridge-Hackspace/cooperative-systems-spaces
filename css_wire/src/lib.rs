//! The **edge ↔ module** vocabulary, on the *local* broker (#219).
//!
//! The server ↔ device wire (`css_lib::wire::kinds`) is namespaced under
//! `{namespace}/devices/{device_id}/` and spoken over the internet or the site
//! link. This is the other wire entirely -- unnamespaced topics on the broker
//! the edge runs, spoken by the ToolGuards, card readers, plugs and sensors in
//! the same building. A module's counterparty is the edge, not the server, so
//! for firmware this is the vocabulary that matters most.
//!
//! **One definition, owned here.** These types were written twice: once in
//! `css_lib::wire` for the edge, and once in spaces-firmware's `spaces-device`
//! for the modules, under different names and kept in step by a contract test on
//! each side (#209, #213). This crate is now the only definition. `css_lib`
//! re-exports it, so nothing in this workspace changed its imports, and
//! spaces-device depends on it by tag. A field changes here, once, and both
//! ends of the wire see it.
//!
//! Before that, the constants lived as string literals in four crates --
//! `edge`, `toolguard-test-ui`, `toolguard-status-ui` and `kiosk` -- each
//! with its own copy and no way to notice when another changed. That is how
//! `toolguard-test-ui` came to be speaking the protocol as it stood before #83
//! and #84, with nothing to say so. `checks/tests/the_firmware_protocol_is_documented.rs`
//! holds `FIRMWARE.md` to this crate in both directions.
//!
//! **What may live here.** Topics, message shapes, and the capability
//! descriptors a device declares at registration ([`capabilities`]) -- what
//! goes over the wire, and the few small rules that belong to a shape (a
//! refusal never carries a strike). Not decisions: what the edge does with a
//! message stays in the edge. And nothing a microcontroller cannot build: serde
//! is the only dependency, because the other end of this wire is an ESP32.

use serde::{Deserialize, Serialize};

pub mod capabilities;

/// A device saying it is alive, on [`local::RESOURCE_ALIVE_REQUEST`] (#170).
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

/// `resource/request/condition` (#213): a device's reading of one of a
/// resource's interlock conditions -- `door_open: true` means the door is
/// open. The plain truth of the named condition; the edge knows which
/// conditions are hazards and which are requirements.
///
/// Until #213 the edge could debounce, gate and trip on a condition but
/// nothing could deliver one: `ModuleState::observe` was reached only from
/// tests, and a `sensor` binding could be alive or gone and nothing more.
/// This is the message that was missing. The edge accepts it only from a
/// device bound on the resource, only for a condition an enabled rule on that
/// resource names, and -- where a rule names a source binding -- only from
/// that binding's device. A reading also counts as being alive.
///
/// Report on every change AND on a timer: a condition never observed denies
/// (`Unobserved`), so a sensor that reports only on change leaves its tool
/// denied until the first change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalCondition {
    /// The device's own id, from registration.
    pub device_id: String,
    /// The resource, by id or external id, as every other local request.
    pub resource: String,
    /// One of the server's condition vocabulary: `door_open`, `flow_ok`, ...
    pub condition: String,
    pub asserted: bool,
}

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
    /// A sensor's reading of one named condition (#213). Request-only, like
    /// `alive`: the lease is the feedback.
    pub const CONDITION_REQUEST: &str = "resource/request/condition";
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
