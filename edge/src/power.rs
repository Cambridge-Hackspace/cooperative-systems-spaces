//! Edge-side power lockout + local fast-trip (#48).
//!
//! Mirrors the `doors.rs` shape: a cache behind an `RwLock`, with the decision
//! logic in plain methods that are unit-tested directly (the "mock firmware
//! facade" — synthetic readings, no broker). Two jobs:
//!
//! * **Fail-secure lockout.** The server pushes a [`PowerStatePayload`] (locked
//!   tool ids + topology). A tool that is locked stays denied here even if the
//!   server becomes unreachable — lockout is **sticky and deny-biased**, the
//!   opposite polarity from the doors open-access latch, which self-expires. A
//!   lockout clears only when a fresh snapshot no longer reports it.
//! * **Local fast-trip.** The edge accumulates the draw firmware reports and
//!   sums it per circuit; a circuit over its amperage limit is tripped locally
//!   (its tools denied immediately) and reported up as an `edge_fast_trip`.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use css_lib::wire::{PowerStatePayload, PowerStateTool};

#[derive(Default)]
struct Inner {
    /// Latest server snapshot; `None` until the first push/poll.
    snapshot: Option<PowerStatePayload>,
    /// Latest draw (amps) per tool, keyed by the tool's server UUID string.
    latest_draw: HashMap<String, (f64, DateTime<Utc>)>,
    /// Circuits this edge tripped locally, held until the server snapshot no
    /// longer reports them locked (sticky — survives a disconnect).
    local_tripped: HashSet<String>,
}

pub struct PowerState {
    inner: RwLock<Inner>,
}

impl Default for PowerState {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerState {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
        }
    }

    /// Whether a circuit is reported locked by a snapshot: it has at least one
    /// tool and every one of them is in `locked_tool_ids`.
    fn circuit_locked(snapshot: &PowerStatePayload, circuit_id: &str) -> bool {
        let locked: HashSet<&str> = snapshot
            .locked_tool_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut any = false;
        for t in snapshot
            .tools
            .iter()
            .filter(|t| t.circuit_id.as_deref() == Some(circuit_id))
        {
            any = true;
            if !locked.contains(t.id.as_str()) {
                return false;
            }
        }
        any
    }

    /// Apply a fresh server snapshot. Drops any local trip the server no longer
    /// reports locked — so a staff re-enable (which clears the server lockout)
    /// also clears the edge-local trip.
    pub fn apply_state(&self, payload: PowerStatePayload) {
        let mut inner = self.inner.write().expect("power lock");
        inner
            .local_tripped
            .retain(|c| Self::circuit_locked(&payload, c));
        inner.snapshot = Some(payload);
    }

    /// Resolve a firmware-supplied tool id (external id, else UUID string) to the
    /// snapshot's canonical tool, if known.
    fn resolve<'a>(snapshot: &'a PowerStatePayload, tool_id: &str) -> Option<&'a PowerStateTool> {
        snapshot
            .tools
            .iter()
            .find(|t| t.external_id.as_deref() == Some(tool_id) || t.id == tool_id)
    }

    /// Is this tool currently locked out? True if it (or its circuit) is in the
    /// last-known server snapshot, OR its circuit was tripped locally. With no
    /// snapshot yet (cold start) it is not locked — the allow-list still gates.
    pub fn is_locked(&self, tool_id: &str) -> bool {
        let inner = self.inner.read().expect("power lock");
        let Some(snap) = &inner.snapshot else {
            return false;
        };
        match Self::resolve(snap, tool_id) {
            Some(tool) => {
                if snap.locked_tool_ids.contains(&tool.id) {
                    return true;
                }
                matches!(&tool.circuit_id, Some(c) if inner.local_tripped.contains(c))
            }
            // Unknown to the power topology: fall back to a raw id match.
            None => snap.locked_tool_ids.iter().any(|id| id == tool_id),
        }
    }

    /// Record firmware's latest draw for a tool (latest-wins), keyed by the
    /// tool's canonical UUID when known.
    pub fn record_draw(&self, tool_id: &str, amps: f64, now: DateTime<Utc>) {
        let mut inner = self.inner.write().expect("power lock");
        let key = inner
            .snapshot
            .as_ref()
            .and_then(|s| Self::resolve(s, tool_id).map(|t| t.id.clone()))
            .unwrap_or_else(|| tool_id.to_string());
        inner.latest_draw.insert(key, (amps, now));
    }

    /// Circuits whose summed latest draw now strictly exceeds their amperage
    /// limit and are not already tripped locally. The caller trips + reports each.
    pub fn evaluate_overages(&self) -> Vec<String> {
        let inner = self.inner.read().expect("power lock");
        let Some(snap) = &inner.snapshot else {
            return Vec::new();
        };
        let tool_circuit: HashMap<&str, &str> = snap
            .tools
            .iter()
            .filter_map(|t| t.circuit_id.as_deref().map(|c| (t.id.as_str(), c)))
            .collect();
        let mut totals: HashMap<&str, f64> = HashMap::new();
        for (tid, (amps, _)) in &inner.latest_draw {
            if let Some(c) = tool_circuit.get(tid.as_str()) {
                *totals.entry(c).or_insert(0.0) += *amps;
            }
        }
        let mut over: Vec<String> = Vec::new();
        for c in &snap.circuits {
            let Ok(limit) = c.amperage_limit.parse::<f64>() else {
                continue;
            };
            let total = totals.get(c.id.as_str()).copied().unwrap_or(0.0);
            if total > limit && !inner.local_tripped.contains(&c.id) {
                over.push(c.id.clone());
            }
        }
        over.sort();
        over
    }

    /// Mark a circuit tripped locally (sticky until a snapshot clears it).
    pub fn mark_tripped(&self, circuit_id: &str) {
        let mut inner = self.inner.write().expect("power lock");
        inner.local_tripped.insert(circuit_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use css_lib::wire::{PowerStateCircuit, PowerStateTool};

    fn tool(id: &str, ext: &str, circuit: Option<&str>) -> PowerStateTool {
        PowerStateTool {
            id: id.to_string(),
            external_id: Some(ext.to_string()),
            circuit_id: circuit.map(str::to_string),
        }
    }
    fn circuit(id: &str, limit: &str) -> PowerStateCircuit {
        PowerStateCircuit {
            id: id.to_string(),
            amperage_limit: limit.to_string(),
        }
    }
    fn payload(
        locked: &[&str],
        circuits: Vec<PowerStateCircuit>,
        tools: Vec<PowerStateTool>,
    ) -> PowerStatePayload {
        PowerStatePayload {
            as_of: "2026-01-01T00:00:00Z".to_string(),
            locked_tool_ids: locked.iter().map(|s| s.to_string()).collect(),
            circuits,
            tools,
        }
    }
    fn now() -> DateTime<Utc> {
        "2026-01-01T00:00:00Z".parse().unwrap()
    }

    #[test]
    fn cold_start_is_not_locked() {
        // No snapshot yet: the allow-list still gates; power says nothing.
        assert!(!PowerState::new().is_locked("t-ext"));
    }

    #[test]
    fn a_locked_tool_is_denied_by_external_id_and_uuid() {
        let ps = PowerState::new();
        ps.apply_state(payload(
            &["t1"],
            vec![],
            vec![tool("t1", "t-ext", Some("c1"))],
        ));
        assert!(ps.is_locked("t-ext"), "matched by external id");
        assert!(ps.is_locked("t1"), "matched by uuid");
        assert!(!ps.is_locked("other"), "an unlisted tool is not locked");
    }

    #[test]
    fn lockout_is_sticky_across_a_disconnect() {
        // Once a lockout is known, it persists with no further snapshots (a
        // disconnected edge keeps denying) — the fail-secure property.
        let ps = PowerState::new();
        ps.apply_state(payload(
            &["t1"],
            vec![],
            vec![tool("t1", "t-ext", Some("c1"))],
        ));
        assert!(ps.is_locked("t-ext"));
        // ...no fresh state arrives (server unreachable)...
        assert!(ps.is_locked("t-ext"), "still denied while disconnected");
    }

    #[test]
    fn a_fresh_snapshot_clears_a_lockout() {
        let ps = PowerState::new();
        ps.apply_state(payload(
            &["t1"],
            vec![],
            vec![tool("t1", "t-ext", Some("c1"))],
        ));
        assert!(ps.is_locked("t-ext"));
        // Staff re-enabled it: the new snapshot no longer lists it.
        ps.apply_state(payload(&[], vec![], vec![tool("t1", "t-ext", Some("c1"))]));
        assert!(!ps.is_locked("t-ext"), "cleared by a fresh snapshot");
    }

    #[test]
    fn overage_trips_only_the_over_limit_circuit() {
        // c1 limit 10, c2 limit 20. Two tools on c1 draw 6+6=12 (over); c2 draws 4.
        let ps = PowerState::new();
        ps.apply_state(payload(
            &[],
            vec![circuit("c1", "10"), circuit("c2", "20")],
            vec![
                tool("a", "a-ext", Some("c1")),
                tool("b", "b-ext", Some("c1")),
                tool("d", "d-ext", Some("c2")),
            ],
        ));
        ps.record_draw("a-ext", 6.0, now());
        ps.record_draw("b-ext", 6.0, now());
        ps.record_draw("d-ext", 4.0, now());
        // Blast radius: only c1 trips; c2 is untouched.
        assert_eq!(ps.evaluate_overages(), vec!["c1".to_string()]);
    }

    #[test]
    fn at_or_under_limit_does_not_trip() {
        let ps = PowerState::new();
        ps.apply_state(payload(
            &[],
            vec![circuit("c1", "10")],
            vec![tool("a", "a-ext", Some("c1"))],
        ));
        ps.record_draw("a-ext", 10.0, now()); // exactly at the limit
        assert!(
            ps.evaluate_overages().is_empty(),
            "at the rating is not over it"
        );
        ps.record_draw("a-ext", 5.0, now()); // latest-wins, well under
        assert!(ps.evaluate_overages().is_empty());
    }

    #[test]
    fn a_tripped_circuit_locks_its_tools_and_is_not_re_reported() {
        let ps = PowerState::new();
        ps.apply_state(payload(
            &[],
            vec![circuit("c1", "10")],
            vec![tool("a", "a-ext", Some("c1"))],
        ));
        ps.record_draw("a-ext", 12.0, now());
        assert_eq!(ps.evaluate_overages(), vec!["c1".to_string()]);
        ps.mark_tripped("c1");
        assert!(
            ps.is_locked("a-ext"),
            "a locally-tripped circuit denies its tools"
        );
        assert!(
            ps.evaluate_overages().is_empty(),
            "an already-tripped circuit is not re-reported"
        );
    }

    #[test]
    fn apply_state_drops_a_local_trip_the_server_cleared() {
        let ps = PowerState::new();
        ps.apply_state(payload(
            &[],
            vec![circuit("c1", "10")],
            vec![tool("a", "a-ext", Some("c1"))],
        ));
        ps.mark_tripped("c1");
        assert!(ps.is_locked("a-ext"));
        // Server confirms the lock: the tool shows up locked -> local trip retained.
        ps.apply_state(payload(
            &["a"],
            vec![circuit("c1", "10")],
            vec![tool("a", "a-ext", Some("c1"))],
        ));
        assert!(ps.is_locked("a-ext"));
        // Staff re-enable: tool no longer locked -> local trip is dropped too.
        ps.apply_state(payload(
            &[],
            vec![circuit("c1", "10")],
            vec![tool("a", "a-ext", Some("c1"))],
        ));
        assert!(
            !ps.is_locked("a-ext"),
            "staff re-enable clears the edge-local trip"
        );
    }
}
