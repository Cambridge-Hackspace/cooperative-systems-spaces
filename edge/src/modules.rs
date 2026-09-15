//! Edge-side tool module coordination + safety interlocks (#83).
//!
//! Mirrors the `power.rs` shape: a cache behind an `RwLock`, with the decision
//! logic in plain methods that are unit-tested directly (synthetic readings, no
//! broker). Three jobs:
//!
//! * **Coordinate modules that are not wired to each other.** A reader on one
//!   device authorizes; a power controller on another switches; a sensor on a
//!   third gates. The edge is what connects them, which is why this logic lives
//!   here rather than in any one module's firmware.
//! * **Evaluate interlocks.** Start gates are AND-ed preconditions; trips are
//!   OR-ed faults. A trip that latches stays denied until an explicit reset --
//!   auto-resuming a cut tool when the lid falls shut is the behaviour the
//!   default exists to prevent.
//! * **Hold the lease.** Authorization to energize is not a fire-and-forget
//!   command but a lease the edge must keep renewing, and it renews only while
//!   every condition still holds. A module that stops hearing renewals fails to
//!   its safe state on its own. The absence of the expected renewal *is* the
//!   trip: an edge that dies, a broker that drops, or a sensor that goes quiet
//!   all stop the renewals, so none of them can leave a tool energized on a
//!   stale promise.
//!
//! What is NOT here: the firmware tier. A module that can inhibit its own relay
//! from a directly-wired sensor should do so, and its own watchdog should fail
//! it safe when it stops hearing from the edge. This file is the cross-module
//! coordinator and the backstop, not a replacement for either.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};
use css_lib::wire::{ToolInterlockRule, ToolModuleStatePayload, ToolModuleTool};

/// Interlock kinds, as they arrive on the wire.
const KIND_START_GATE: &str = "start_gate";
const KIND_TRIP: &str = "trip";

/// Module roles.
const ROLE_POWER: &str = "power";

/// Disconnect policies.
const ON_DISCONNECT_FAIL_OFF: &str = "fail_off";

/// Whether a condition names a hazard (asserted = unsafe) or a requirement
/// (asserted = safe).
///
/// The distinction is what lets one vocabulary serve both interlock kinds: a
/// start gate is satisfied when a hazard is clear or a requirement is met, and a
/// trip fires when a hazard appears or a requirement is lost (coolant flow
/// stopping mid-cut is a trip, not merely a failure to start).
///
/// An unrecognised condition is treated as a hazard. A rule this edge does not
/// understand must not be the reason a tool stays energized.
fn is_hazard(condition: &str) -> bool {
    !matches!(condition, "flow_ok" | "authorized")
}

/// Why a tool may not be energized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denial {
    /// A start gate is not satisfied.
    Gate(String),
    /// A trip fired and latched; it needs an explicit reset.
    Latched(String),
    /// A trip is currently firing.
    Tripped(String),
    /// A condition has never been observed, so it cannot be confirmed safe.
    Unobserved(String),
    /// A module the tool depends on has gone quiet and its policy is fail_off.
    ModuleOffline(String),
    /// Nothing is known about this tool at all.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(Denial),
}

/// What the watchdog decided about one power module's lease this pass.
///
/// `grant: false` is not "do nothing" -- it is an instruction to drop the lease,
/// which a module treats as de-energize. A module that receives nothing at all
/// must reach the same state on its own once its lease runs out; that redundancy
/// is the point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseAction {
    pub tool_id: String,
    pub device_id: String,
    pub grant: bool,
    /// How long the grant is good for. The module is expected to de-energize on
    /// its own if it is not renewed within this window.
    pub ttl_ms: i64,
    /// Why the lease was refused, for the log and the audit trail.
    pub reason: Option<String>,
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// One condition's latest reading for one tool.
#[derive(Debug, Clone, Copy)]
struct Reading {
    asserted: bool,
    /// When the value last changed -- the basis for debounce.
    since: DateTime<Utc>,
}

#[derive(Default)]
struct Inner {
    /// Latest wiring snapshot; `None` until the first push/poll.
    snapshot: Option<ToolModuleStatePayload>,
    /// (tool_id, condition) -> latest reading.
    readings: HashMap<(String, String), Reading>,
    /// Latched trips, tool_id -> the condition that fired. Survives a snapshot
    /// replacement: a latch is safety state, not configuration.
    latched: HashMap<String, String>,
    /// tool_id -> when its power lease expires.
    leases: HashMap<String, DateTime<Utc>>,
    /// module_id -> when it was last heard from.
    module_seen: HashMap<String, DateTime<Utc>>,
}

pub struct ModuleState {
    inner: RwLock<Inner>,
}

impl Default for ModuleState {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleState {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
        }
    }

    /// Replace the wiring snapshot. Latches, readings and leases are kept: the
    /// snapshot describes configuration, and discarding live safety state
    /// because an admin renamed a binding would clear a latch the tool is
    /// relying on.
    pub fn apply_state(&self, payload: ToolModuleStatePayload) {
        let mut inner = self.inner.write().expect("module lock");
        inner.snapshot = Some(payload);
    }

    pub fn apply_state_bytes(&self, bytes: &[u8]) -> Result<(), serde_json::Error> {
        let payload: ToolModuleStatePayload = serde_json::from_slice(bytes)?;
        self.apply_state(payload);
        Ok(())
    }

    pub fn has_state(&self) -> bool {
        self.inner.read().expect("module lock").snapshot.is_some()
    }

    /// Record a sensor reading. `since` only moves when the value changes, so a
    /// condition that has been steady for a while satisfies its debounce and one
    /// that keeps flapping never does.
    pub fn observe(&self, tool_id: &str, condition: &str, asserted: bool, now: DateTime<Utc>) {
        let mut inner = self.inner.write().expect("module lock");
        let key = (tool_id.to_string(), condition.to_string());
        match inner.readings.get(&key) {
            Some(prev) if prev.asserted == asserted => {}
            _ => {
                inner.readings.insert(
                    key,
                    Reading {
                        asserted,
                        since: now,
                    },
                );
            }
        }
    }

    /// Note that a module reported in.
    pub fn note_module_seen(&self, module_id: &str, now: DateTime<Utc>) {
        let mut inner = self.inner.write().expect("module lock");
        inner.module_seen.insert(module_id.to_string(), now);
    }

    /// Clear a latched trip. `re_auth` and `operator_ack` both land here; the
    /// difference is who is allowed to call it, which is the caller's business.
    pub fn reset_latch(&self, tool_id: &str) {
        let mut inner = self.inner.write().expect("module lock");
        inner.latched.remove(tool_id);
    }

    pub fn is_latched(&self, tool_id: &str) -> bool {
        self.inner
            .read()
            .expect("module lock")
            .latched
            .contains_key(tool_id)
    }

    /// Find a tool's wiring by server UUID or `external_id`, matching the
    /// allow-list's resolution order.
    fn resolve<'a>(snapshot: &'a ToolModuleStatePayload, tool: &str) -> Option<&'a ToolModuleTool> {
        snapshot
            .tools
            .iter()
            .find(|t| t.tool_id == tool)
            .or_else(|| {
                snapshot
                    .tools
                    .iter()
                    .find(|t| t.external_id.as_deref() == Some(tool))
            })
    }

    /// Whether a rule's condition currently counts as satisfied for `kind`.
    ///
    /// Returns `None` when the condition has never been observed: that is not
    /// "safe", it is "unknown", and the caller denies on it.
    fn satisfied(
        inner: &Inner,
        tool_key: &str,
        rule: &ToolInterlockRule,
        now: DateTime<Utc>,
    ) -> Option<bool> {
        let reading = inner
            .readings
            .get(&(tool_key.to_string(), rule.condition.clone()))?;

        // A change that has not yet outlasted its debounce keeps the previous
        // meaning: for a hazard that means the hazard is still assumed present.
        let settled = now - reading.since >= Duration::milliseconds(rule.debounce_ms.max(0) as i64);
        let effective = if settled {
            reading.asserted
        } else {
            !reading.asserted
        };

        Some(if is_hazard(&rule.condition) {
            !effective
        } else {
            effective
        })
    }

    /// Modules bound to this tool that have gone quiet past `timeout` and whose
    /// disconnect policy is to fail off.
    fn offline_fail_off(
        inner: &Inner,
        tool: &ToolModuleTool,
        now: DateTime<Utc>,
        timeout: Duration,
    ) -> Option<String> {
        tool.modules
            .iter()
            .find(|m| {
                if m.on_disconnect != ON_DISCONNECT_FAIL_OFF {
                    return false;
                }
                match inner.module_seen.get(&m.id) {
                    // Never heard from at all: a module that has not checked in
                    // cannot be confirmed healthy, and this policy says that is
                    // a reason to stay off.
                    None => true,
                    Some(seen) => now - *seen > timeout,
                }
            })
            .map(|m| m.name.clone())
    }

    /// The interlock engine: may this tool be energized right now?
    ///
    /// Start gates are AND-ed, trips are OR-ed, a latched trip denies until
    /// reset, an unobserved condition denies, and a fail-off module that has
    /// gone quiet denies.
    pub fn evaluate(&self, tool: &str, now: DateTime<Utc>, module_timeout: Duration) -> Decision {
        let inner = self.inner.read().expect("module lock");
        let Some(snapshot) = inner.snapshot.as_ref() else {
            // Cold start: nothing is known, so nothing is allowed. Same polarity
            // as the allow-list's cold start.
            return Decision::Deny(Denial::Unknown);
        };
        let Some(wiring) = Self::resolve(snapshot, tool) else {
            // A tool with no wiring has no interlocks to satisfy. It is not this
            // subsystem's business, so it does not deny it.
            return Decision::Allow;
        };
        let tool_key = wiring.tool_id.clone();

        if let Some(condition) = inner.latched.get(&tool_key) {
            return Decision::Deny(Denial::Latched(condition.clone()));
        }

        if let Some(name) = Self::offline_fail_off(&inner, wiring, now, module_timeout) {
            return Decision::Deny(Denial::ModuleOffline(name));
        }

        for rule in &wiring.interlocks {
            match Self::satisfied(&inner, &tool_key, rule, now) {
                None => return Decision::Deny(Denial::Unobserved(rule.condition.clone())),
                Some(true) => {}
                Some(false) => {
                    return match rule.kind.as_str() {
                        KIND_TRIP => Decision::Deny(Denial::Tripped(rule.condition.clone())),
                        KIND_START_GATE => Decision::Deny(Denial::Gate(rule.condition.clone())),
                        // An unrecognised kind is treated as a gate rather than
                        // ignored: a rule this edge cannot classify must not be
                        // the reason a tool is allowed on.
                        _ => Decision::Deny(Denial::Gate(rule.condition.clone())),
                    };
                }
            }
        }

        Decision::Allow
    }

    /// Re-evaluate and either renew the lease or let it lapse.
    ///
    /// This is the whole safety argument in one method: a lease is extended only
    /// while `evaluate` still allows, so anything that stops this being called --
    /// a dead edge, a dropped broker, a sensor gone quiet -- lets every lease
    /// expire and every module fall back to its own safe state.
    ///
    /// A firing trip that latches is recorded here, so it keeps denying after the
    /// condition clears.
    pub fn renew(
        &self,
        tool: &str,
        now: DateTime<Utc>,
        ttl: Duration,
        module_timeout: Duration,
    ) -> Decision {
        let decision = self.evaluate(tool, now, module_timeout);
        let mut inner = self.inner.write().expect("module lock");

        let tool_key = inner
            .snapshot
            .as_ref()
            .and_then(|s| Self::resolve(s, tool))
            .map(|t| t.tool_id.clone())
            .unwrap_or_else(|| tool.to_string());

        match &decision {
            Decision::Allow => {
                inner.leases.insert(tool_key, now + ttl);
            }
            Decision::Deny(denial) => {
                inner.leases.remove(&tool_key);
                // Latch a trip so clearing the condition is not enough to resume.
                if let Denial::Tripped(condition) = denial {
                    let latches = inner
                        .snapshot
                        .as_ref()
                        .and_then(|s| Self::resolve(s, tool))
                        .map(|t| {
                            t.interlocks.iter().any(|r| {
                                r.kind == KIND_TRIP && &r.condition == condition && r.latch
                            })
                        })
                        .unwrap_or(false);
                    if latches {
                        inner.latched.insert(tool_key, condition.clone());
                    }
                }
            }
        }
        decision
    }

    /// Whether a tool's lease is currently valid. A module should be energized
    /// only while this holds.
    pub fn lease_valid(&self, tool: &str, now: DateTime<Utc>) -> bool {
        let inner = self.inner.read().expect("module lock");
        let tool_key = inner
            .snapshot
            .as_ref()
            .and_then(|s| Self::resolve(s, tool))
            .map(|t| t.tool_id.clone())
            .unwrap_or_else(|| tool.to_string());
        inner
            .leases
            .get(&tool_key)
            .is_some_and(|until| *until > now)
    }

    /// Power modules bound to a tool, for addressing lease commands.
    pub fn power_modules(&self, tool: &str) -> Vec<String> {
        let inner = self.inner.read().expect("module lock");
        inner
            .snapshot
            .as_ref()
            .and_then(|s| Self::resolve(s, tool))
            .map(|t| {
                t.modules
                    .iter()
                    .filter(|m| m.role == ROLE_POWER)
                    .map(|m| m.device_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One pass of the watchdog: re-evaluate every wired tool and say what
    /// should happen to each power module's lease.
    ///
    /// Pure in the sense that matters -- it mutates lease/latch state but
    /// performs no I/O and returns the commands rather than sending them, so the
    /// decision can be tested without a broker. The caller is responsible for
    /// delivering them.
    ///
    /// A tool with no power module yields no action: there is nothing to hold
    /// on, so there is nothing to renew.
    pub fn tick(
        &self,
        now: DateTime<Utc>,
        ttl: Duration,
        module_timeout: Duration,
    ) -> Vec<LeaseAction> {
        let mut out = Vec::new();
        for tool in self.tools() {
            let decision = self.renew(&tool, now, ttl, module_timeout);
            for device_id in self.power_modules(&tool) {
                out.push(LeaseAction {
                    tool_id: tool.clone(),
                    device_id,
                    grant: decision.is_allowed(),
                    ttl_ms: ttl.num_milliseconds().max(0),
                    reason: match &decision {
                        Decision::Allow => None,
                        Decision::Deny(d) => Some(format!("{d:?}")),
                    },
                });
            }
        }
        out
    }

    /// Every tool this edge holds wiring for.
    pub fn tools(&self) -> Vec<String> {
        let inner = self.inner.read().expect("module lock");
        inner
            .snapshot
            .as_ref()
            .map(|s| s.tools.iter().map(|t| t.tool_id.clone()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use css_lib::wire::{ToolInterlockRule, ToolModuleBinding, ToolModuleTool};

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-15T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn timeout() -> Duration {
        Duration::seconds(30)
    }

    fn rule(kind: &str, condition: &str, latch: bool, debounce_ms: i32) -> ToolInterlockRule {
        ToolInterlockRule {
            id: format!("r-{kind}-{condition}"),
            kind: kind.to_string(),
            condition: condition.to_string(),
            source_module_id: None,
            debounce_ms,
            latch,
            reset: "re_auth".to_string(),
            enforcement: "edge".to_string(),
        }
    }

    fn binding(id: &str, role: &str, on_disconnect: &str) -> ToolModuleBinding {
        ToolModuleBinding {
            id: id.to_string(),
            device_id: format!("dev-{id}"),
            role: role.to_string(),
            name: format!("{role} {id}"),
            params: serde_json::json!({}),
            on_disconnect: on_disconnect.to_string(),
        }
    }

    fn state_with(
        modules: Vec<ToolModuleBinding>,
        interlocks: Vec<ToolInterlockRule>,
    ) -> ModuleState {
        let s = ModuleState::new();
        s.apply_state(ToolModuleStatePayload {
            as_of: t0().to_rfc3339(),
            tools: vec![ToolModuleTool {
                tool_id: "tool-1".to_string(),
                external_id: Some("ext-1".to_string()),
                modules,
                interlocks,
                power_fails_safe: true,
            }],
        });
        s
    }

    /// Every module is healthy, so module liveness is never the reason a test
    /// below denies.
    fn all_seen(s: &ModuleState, ids: &[&str], now: DateTime<Utc>) {
        for id in ids {
            s.note_module_seen(id, now);
        }
    }

    #[test]
    fn cold_start_denies_to_be_safe() {
        let s = ModuleState::new();
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::Unknown)
        );
    }

    #[test]
    fn a_tool_with_no_wiring_is_not_this_subsystems_business() {
        let s = state_with(vec![], vec![]);
        // Not the tool in the snapshot: no bindings, no interlocks, no opinion.
        assert_eq!(
            s.evaluate("some-other-tool", t0(), timeout()),
            Decision::Allow
        );
    }

    #[test]
    fn an_unobserved_condition_denies_rather_than_assuming_safe() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::Unobserved("door_open".to_string()))
        );
    }

    #[test]
    fn a_clear_hazard_allows_and_an_asserted_one_trips() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());
        assert_eq!(s.evaluate("tool-1", t0(), timeout()), Decision::Allow);

        s.observe("tool-1", "door_open", true, t0());
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::Tripped("door_open".to_string()))
        );
    }

    #[test]
    fn a_requirement_is_the_opposite_polarity_of_a_hazard() {
        // flow_ok asserted means coolant IS flowing, which is the safe state.
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "flow_ok", false, 0)],
        );
        s.observe("tool-1", "flow_ok", true, t0());
        assert_eq!(s.evaluate("tool-1", t0(), timeout()), Decision::Allow);

        // Flow stops mid-cut: that is a trip, not merely a failure to start.
        s.observe("tool-1", "flow_ok", false, t0());
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::Tripped("flow_ok".to_string()))
        );
    }

    #[test]
    fn start_gates_are_anded() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![
                rule(KIND_START_GATE, "door_open", false, 0),
                rule(KIND_START_GATE, "estop", false, 0),
            ],
        );
        s.observe("tool-1", "door_open", false, t0());
        // The second gate is still unobserved, so the AND cannot be satisfied.
        assert!(!s.evaluate("tool-1", t0(), timeout()).is_allowed());

        s.observe("tool-1", "estop", false, t0());
        assert_eq!(s.evaluate("tool-1", t0(), timeout()), Decision::Allow);

        // Either one failing is enough to deny.
        s.observe("tool-1", "estop", true, t0());
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::Gate("estop".to_string()))
        );
    }

    #[test]
    fn a_debounced_hazard_is_assumed_present_until_it_settles() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", false, 500)],
        );
        // Door reads closed, but the reading is younger than its debounce: the
        // previous meaning holds, and for a hazard that is "still present".
        s.observe("tool-1", "door_open", false, t0());
        assert!(!s.evaluate("tool-1", t0(), timeout()).is_allowed());

        // Once it has been steady past the debounce window, it counts.
        let later = t0() + Duration::milliseconds(600);
        assert_eq!(s.evaluate("tool-1", later, timeout()), Decision::Allow);
    }

    #[test]
    fn a_latching_trip_keeps_denying_after_the_condition_clears() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());
        assert!(s
            .renew("tool-1", t0(), Duration::seconds(30), timeout())
            .is_allowed());

        // Door opens mid-run: the trip fires and latches.
        s.observe("tool-1", "door_open", true, t0());
        let d = s.renew("tool-1", t0(), Duration::seconds(30), timeout());
        assert_eq!(d, Decision::Deny(Denial::Tripped("door_open".to_string())));
        assert!(s.is_latched("tool-1"));
        assert!(
            !s.lease_valid("tool-1", t0()),
            "the lease must lapse on a trip"
        );

        // Door shuts again. Closing the lid must NOT restart the laser.
        s.observe("tool-1", "door_open", false, t0());
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::Latched("door_open".to_string()))
        );

        // Only an explicit reset clears it.
        s.reset_latch("tool-1");
        assert_eq!(s.evaluate("tool-1", t0(), timeout()), Decision::Allow);
    }

    #[test]
    fn a_non_latching_trip_recovers_when_the_condition_clears() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", false, 0)],
        );
        s.observe("tool-1", "door_open", true, t0());
        assert!(!s
            .renew("tool-1", t0(), Duration::seconds(30), timeout())
            .is_allowed());
        assert!(!s.is_latched("tool-1"));

        s.observe("tool-1", "door_open", false, t0());
        assert_eq!(s.evaluate("tool-1", t0(), timeout()), Decision::Allow);
    }

    #[test]
    fn a_lease_expires_when_it_stops_being_renewed() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());
        s.renew("tool-1", t0(), Duration::seconds(30), timeout());
        assert!(s.lease_valid("tool-1", t0() + Duration::seconds(29)));
        // Nobody renewed. This is the dead-man's switch: the edge dying, the
        // broker dropping, or this loop stalling all look like exactly this.
        assert!(!s.lease_valid("tool-1", t0() + Duration::seconds(31)));
    }

    #[test]
    fn a_fail_off_module_gone_quiet_denies() {
        let s = state_with(
            vec![binding("m1", "power", ON_DISCONNECT_FAIL_OFF)],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());
        // Never heard from: not confirmable as healthy, so not allowed.
        assert_eq!(
            s.evaluate("tool-1", t0(), timeout()),
            Decision::Deny(Denial::ModuleOffline("power m1".to_string()))
        );

        all_seen(&s, &["m1"], t0());
        assert_eq!(s.evaluate("tool-1", t0(), timeout()), Decision::Allow);

        // It goes quiet past the timeout.
        let much_later = t0() + Duration::seconds(31);
        assert_eq!(
            s.evaluate("tool-1", much_later, timeout()),
            Decision::Deny(Denial::ModuleOffline("power m1".to_string()))
        );
    }

    #[test]
    fn a_hold_last_module_gone_quiet_does_not_itself_deny() {
        // The counterpart to the test above, and the reason the policy is
        // per-module: a wifi blip must not cut a three-hour job on a tool whose
        // safety does not depend on the link.
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());
        let much_later = t0() + Duration::seconds(600);
        assert_eq!(s.evaluate("tool-1", much_later, timeout()), Decision::Allow);
    }

    #[test]
    fn a_tool_resolves_by_external_id_too() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());
        assert_eq!(s.evaluate("ext-1", t0(), timeout()), Decision::Allow);
        // And the lease is keyed on the canonical id, not whichever alias was
        // used, so renewing by one and checking by the other agrees.
        s.renew("ext-1", t0(), Duration::seconds(30), timeout());
        assert!(s.lease_valid("tool-1", t0()));
    }

    #[test]
    fn an_unrecognised_condition_is_treated_as_a_hazard() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(
                KIND_TRIP,
                "something_this_edge_has_never_heard_of",
                true,
                0,
            )],
        );
        s.observe(
            "tool-1",
            "something_this_edge_has_never_heard_of",
            true,
            t0(),
        );
        assert!(
            !s.evaluate("tool-1", t0(), timeout()).is_allowed(),
            "an unknown condition must not be the reason a tool is allowed on"
        );
    }

    #[test]
    fn power_modules_are_addressable_for_lease_commands() {
        let s = state_with(
            vec![
                binding("m1", "power", "fail_off"),
                binding("m2", "reader", "ignore"),
                binding("m3", "sensor", "ignore"),
            ],
            vec![],
        );
        assert_eq!(s.power_modules("tool-1"), vec!["dev-m1".to_string()]);
    }

    #[test]
    fn a_tick_grants_while_safe_and_revokes_the_moment_it_is_not() {
        let s = state_with(
            vec![
                binding("m1", "power", "hold_last"),
                binding("m2", "sensor", "ignore"),
            ],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", false, t0());

        let actions = s.tick(t0(), Duration::seconds(30), timeout());
        // One action, for the one power module -- the sensor is not something a
        // lease is held on.
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].device_id, "dev-m1");
        assert!(actions[0].grant);
        assert_eq!(actions[0].ttl_ms, 30_000);
        assert!(s.lease_valid("tool-1", t0()));

        // The door opens.
        s.observe("tool-1", "door_open", true, t0());
        let actions = s.tick(t0(), Duration::seconds(30), timeout());
        assert_eq!(actions.len(), 1);
        assert!(!actions[0].grant, "an open door must revoke the lease");
        assert!(actions[0].reason.is_some(), "a refusal must say why");
        assert!(!s.lease_valid("tool-1", t0()));
    }

    #[test]
    fn a_tool_with_no_power_module_yields_no_lease_action() {
        let s = state_with(vec![binding("m2", "sensor", "ignore")], vec![]);
        assert!(s.tick(t0(), Duration::seconds(30), timeout()).is_empty());
    }

    #[test]
    fn a_snapshot_replacement_does_not_clear_a_latch() {
        let s = state_with(
            vec![binding("m1", "power", "hold_last")],
            vec![rule(KIND_TRIP, "door_open", true, 0)],
        );
        s.observe("tool-1", "door_open", true, t0());
        s.renew("tool-1", t0(), Duration::seconds(30), timeout());
        assert!(s.is_latched("tool-1"));

        // An admin edits an unrelated binding; the snapshot is replaced.
        s.apply_state(ToolModuleStatePayload {
            as_of: t0().to_rfc3339(),
            tools: vec![ToolModuleTool {
                tool_id: "tool-1".to_string(),
                external_id: Some("ext-1".to_string()),
                modules: vec![binding("m1", "power", "hold_last")],
                interlocks: vec![rule(KIND_TRIP, "door_open", true, 0)],
                power_fails_safe: true,
            }],
        });
        assert!(
            s.is_latched("tool-1"),
            "a configuration change must not clear live safety state"
        );
    }
}
