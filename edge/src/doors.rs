//! Door access — types and edge-side decision logic.
//!
//! The server compiles each device's per-door allow/deny lists and publishes
//! them to `{namespace}/devices/{device_id}/doors/state`. The edge keeps the
//! latest snapshot in memory and decides RFID scans locally — fast and
//! resilient to server outages.
//!
//! ## MQTT contract (mirrors `server/src/doors.rs` and the API spec)
//!
//! **Remote, server → edge** (subscribed by the edge):
//! - `{namespace}/devices/{device_id}/doors/state`   JSON [`DoorStateSnapshot`]
//! - `{namespace}/devices/{device_id}/doors/unlock`  JSON [`UnlockCommand`]
//!
//! **Remote, edge → server** (subscribed by the server):
//! - `{namespace}/devices/{device_id}/doors/event`   JSON [`DoorsEvent`]
//!
//! **Local, hardware ↔ edge** (separate broker that runs the relay/reader):
//! - in:  `door/request/scan`    JSON [`LocalScanRequest`]
//! - out: `door/response/unlock` JSON [`LocalUnlockResponse`]
//!
//! ## TODO (hardware bridge integration)
//!
//! Wiring this into [`crate::mqtt::EdgeMqttClient`] (remote `subscribe_to_commands`)
//! and [`crate::mqtt::LocalMqttClient`] (local `subscribe_to_requests`) is left
//! as a small follow-up — it mirrors what `toolguard` already does there.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledDoor {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub unlock_duration_ms: i32,
    #[serde(default)]
    pub allow_cards: Vec<String>,
    #[serde(default)]
    pub deny_cards: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoorStateSnapshot {
    pub snapshot_at: DateTime<Utc>,
    pub doors: Vec<CompiledDoor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockCommand {
    pub door_id: Uuid,
    pub duration_ms: i32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalScanRequest {
    pub door_id: Uuid,
    pub card_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalUnlockResponse {
    pub door_id: Uuid,
    pub granted: bool,
    pub duration_ms: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoorsEvent {
    pub door_id: Uuid,
    pub card_id: Option<String>,
    pub granted: bool,
    pub reason: Option<String>,
    pub source: &'static str,
    pub occurred_at: DateTime<Utc>,
}

/// Decision returned by [`DoorsState::decide`].
#[derive(Debug, Clone)]
pub enum Decision {
    Allow { duration_ms: i32 },
    Deny(&'static str),
}

/// In-memory state cache fed by `doors/state` snapshots and read by the
/// scan handler.
#[derive(Debug, Default)]
pub struct DoorsState {
    inner: RwLock<Vec<CompiledDoor>>,
}

impl DoorsState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Replace the cache with a fresh snapshot.
    pub fn apply_snapshot(&self, snapshot: DoorStateSnapshot) {
        let mut w = self.inner.write().expect("doors state poisoned");
        *w = snapshot.doors;
    }

    /// Decide an RFID scan against the current cache. Deny beats allow.
    pub fn decide(&self, door_id: Uuid, card_id: &str) -> Decision {
        let guard = self.inner.read().expect("doors state poisoned");
        let door = match guard.iter().find(|d| d.id == door_id) {
            Some(d) => d,
            None => return Decision::Deny("Unknown door"),
        };
        if !door.enabled {
            return Decision::Deny("Door disabled");
        }
        if door.deny_cards.iter().any(|c| c == card_id) {
            return Decision::Deny("Card denied");
        }
        if door.allow_cards.iter().any(|c| c == card_id) {
            return Decision::Allow {
                duration_ms: door.unlock_duration_ms,
            };
        }
        Decision::Deny("Card not authorized")
    }

    /// Number of doors currently cached. Useful for `/status` / logging.
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn snap(door_id: Uuid, allow: &[&str], deny: &[&str]) -> DoorStateSnapshot {
        DoorStateSnapshot {
            snapshot_at: Utc::now(),
            doors: vec![CompiledDoor {
                id: door_id,
                name: "Front".into(),
                enabled: true,
                unlock_duration_ms: 4200,
                allow_cards: allow.iter().map(|s| s.to_string()).collect(),
                deny_cards: deny.iter().map(|s| s.to_string()).collect(),
            }],
        }
    }

    #[test]
    fn allow_when_listed() {
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        s.apply_snapshot(snap(id, &["A1", "B2"], &[]));
        match s.decide(id, "A1") {
            Decision::Allow { duration_ms } => assert_eq!(duration_ms, 4200),
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn deny_beats_allow() {
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        s.apply_snapshot(snap(id, &["A1"], &["A1"]));
        assert!(matches!(s.decide(id, "A1"), Decision::Deny(_)));
    }

    #[test]
    fn unknown_card_denied() {
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        s.apply_snapshot(snap(id, &["A1"], &[]));
        assert!(matches!(s.decide(id, "ZZ"), Decision::Deny(_)));
    }

    #[test]
    fn disabled_door_denied_even_for_allowed_card() {
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        let mut snap = snap(id, &["A1"], &[]);
        snap.doors[0].enabled = false;
        s.apply_snapshot(snap);
        assert!(matches!(s.decide(id, "A1"), Decision::Deny(_)));
    }
}
