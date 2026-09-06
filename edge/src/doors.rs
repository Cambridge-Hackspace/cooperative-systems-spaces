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
//! ## Open Access hold (issue #12)
//!
//! An Open Access window holds the strike unlocked for its whole duration. The
//! external relay firmware only understands the momentary `door/response/unlock`
//! (a `duration_ms` pulse it auto-relocks after), so the edge *holds* a door by
//! re-sending that pulse faster than it elapses — see [`DoorsState::hold_pulses_at`]
//! / [`hold_pulse_ms`] and the refresh loop in `main.rs`. It is driven off this
//! device's own clock and simply stops at the window end, so the strike
//! auto-relocks on its own if a window closes, the edge dies, or the server
//! link drops (fail-secure + edge-local expiry).

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
    /// When set, the strike is held unlocked (no card required) until this
    /// instant — the server's Open Access latch. `#[serde(default)]` keeps the
    /// wire back-compatible: a server that predates this field omits it → `None`
    /// → normal card-gated behavior.
    #[serde(default)]
    pub hold_unlock_until: Option<DateTime<Utc>>,
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

/// How often the edge re-sends the momentary unlock to keep an Open Access door
/// held open. Must be shorter than the pulse [`hold_pulse_ms`] returns so the
/// strike never de-energizes between refreshes.
pub const HOLD_REFRESH_SECS: u64 = 30;

/// Extra time each hold pulse outlasts the refresh interval, absorbing tick
/// jitter so the relay never drops mid-window. The pulse is still capped at the
/// window's remaining time, so the strike relocks *at* the boundary, not after.
pub const HOLD_PULSE_MARGIN_SECS: i64 = 15;

/// The momentary-unlock `duration_ms` to publish *now* to keep an Open Access
/// door held open, or `None` if it must not be held (no window, or the window
/// has ended as of `now`).
///
/// The pulse outlasts `refresh` (by [`HOLD_PULSE_MARGIN_SECS`]) so a continuous
/// refresh never lets the strike drop, but is capped at the time remaining until
/// `hold_unlock_until` so the last pulse expires at the window boundary — the
/// strike then auto-relocks with no further message. Comparing to `now` (the
/// caller passes this device's own clock) is what self-expires the hold on a
/// server disconnect. Pure, so it is unit-tested directly.
pub fn hold_pulse_ms(
    hold_unlock_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    refresh: chrono::Duration,
) -> Option<i32> {
    let until = hold_unlock_until?;
    let remaining = until - now;
    if remaining <= chrono::Duration::zero() {
        return None;
    }
    let pulse = (refresh + chrono::Duration::seconds(HOLD_PULSE_MARGIN_SECS)).min(remaining);
    // Capped at 45s (refresh + margin) so this never overflows i32; floored at
    // 1ms so a sub-millisecond tail still reads as a (near-instant) unlock
    // rather than the `0` the wire uses for "denied".
    Some(pulse.num_milliseconds().max(1) as i32)
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

    /// Decide an RFID scan against the current cache, as of now. Deny beats
    /// allow. See [`Self::decide_at`] for the time-parameterized core.
    pub fn decide(&self, door_id: Uuid, card_id: &str) -> Decision {
        self.decide_at(door_id, card_id, Utc::now())
    }

    /// Decide a scan as of `now`. Split out from [`Self::decide`] so the Open
    /// Access held-unlock window is testable against a fixed clock (and so the
    /// golden vectors, whose `now` is frozen, drive the same code production
    /// runs). Order matters: an explicit `deny_cards` ban wins even during an
    /// open window, but otherwise a live held-unlock admits any card.
    pub fn decide_at(&self, door_id: Uuid, card_id: &str, now: DateTime<Utc>) -> Decision {
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
        // Open Access: while the held-unlock window is live by *this device's*
        // clock, any card enters. Comparing to the local clock is what makes the
        // latch self-expire on disconnect (fail-secure + edge-local expiry): a
        // stale snapshot cannot hold the door open past `hold_unlock_until`.
        if let Some(until) = door.hold_unlock_until {
            if now < until {
                return Decision::Allow {
                    duration_ms: door.unlock_duration_ms,
                };
            }
        }
        if door.allow_cards.iter().any(|c| c == card_id) {
            return Decision::Allow {
                duration_ms: door.unlock_duration_ms,
            };
        }
        Decision::Deny("Card not authorized")
    }

    /// The doors that should be held open right now and the momentary-unlock
    /// `duration_ms` to (re)send each, as of `now`. The refresh loop in `main.rs`
    /// calls this every [`HOLD_REFRESH_SECS`] and publishes an unlock for each.
    /// A disabled door is never held (mirrors [`Self::decide_at`]).
    pub fn hold_pulses_at(
        &self,
        now: DateTime<Utc>,
        refresh: chrono::Duration,
    ) -> Vec<(Uuid, i32)> {
        let guard = self.inner.read().expect("doors state poisoned");
        guard
            .iter()
            .filter(|d| d.enabled)
            .filter_map(|d| hold_pulse_ms(d.hold_unlock_until, now, refresh).map(|ms| (d.id, ms)))
            .collect()
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
        snap_held(door_id, allow, deny, None)
    }

    fn snap_held(
        door_id: Uuid,
        allow: &[&str],
        deny: &[&str],
        hold_unlock_until: Option<DateTime<Utc>>,
    ) -> DoorStateSnapshot {
        DoorStateSnapshot {
            snapshot_at: Utc::now(),
            doors: vec![CompiledDoor {
                id: door_id,
                name: "Front".into(),
                enabled: true,
                unlock_duration_ms: 4200,
                allow_cards: allow.iter().map(|s| s.to_string()).collect(),
                deny_cards: deny.iter().map(|s| s.to_string()).collect(),
                hold_unlock_until,
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

    #[test]
    fn open_access_admits_any_card_while_window_is_live() {
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        let now = Utc::now();
        // Held open until an hour from `now`; no card is on any list.
        s.apply_snapshot(snap_held(
            id,
            &[],
            &[],
            Some(now + chrono::Duration::hours(1)),
        ));
        // A card nobody authorized still gets in during the window.
        match s.decide_at(id, "STRANGER", now) {
            Decision::Allow { duration_ms } => assert_eq!(duration_ms, 4200),
            other => panic!("expected Allow during open window, got {other:?}"),
        }
    }

    #[test]
    fn open_access_self_expires_from_local_clock() {
        // The fail-secure guarantee: once `now` passes `hold_unlock_until`, the
        // latch is dead even though no fresh snapshot (no closing push) ever
        // arrived. An unlisted card is then denied.
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        let now = Utc::now();
        s.apply_snapshot(snap_held(
            id,
            &[],
            &[],
            Some(now - chrono::Duration::seconds(1)),
        ));
        assert!(matches!(
            s.decide_at(id, "STRANGER", now),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn hold_pulse_is_none_without_a_window_or_after_it_ends() {
        let now = Utc::now();
        let refresh = chrono::Duration::seconds(30);
        // No window at all.
        assert_eq!(hold_pulse_ms(None, now, refresh), None);
        // Window already ended -> no pulse. This is the fail-secure boundary:
        // the loop stops sending and the strike auto-relocks.
        assert_eq!(
            hold_pulse_ms(Some(now - chrono::Duration::seconds(1)), now, refresh),
            None
        );
    }

    #[test]
    fn hold_pulse_outlasts_the_refresh_mid_window() {
        // Far from the boundary, the pulse is refresh + margin (45s), so a pulse
        // every 30s always overlaps the previous one and the strike never drops.
        let now = Utc::now();
        let refresh = chrono::Duration::seconds(30);
        let ms = hold_pulse_ms(Some(now + chrono::Duration::hours(2)), now, refresh).unwrap();
        assert_eq!(ms, 45_000);
        assert!(
            ms > refresh.num_milliseconds() as i32,
            "a mid-window pulse must outlast the refresh interval or the strike drops between refreshes"
        );
    }

    #[test]
    fn hold_pulse_is_capped_at_the_window_boundary() {
        // Near the end, the pulse shrinks to exactly the time left, so the last
        // pulse expires at the boundary rather than holding the door open past it.
        let now = Utc::now();
        let refresh = chrono::Duration::seconds(30);
        let ms = hold_pulse_ms(Some(now + chrono::Duration::seconds(10)), now, refresh).unwrap();
        assert!(
            (9_000..=10_000).contains(&ms),
            "expected ~10s pulse capped at the remaining window, got {ms}ms"
        );
    }

    #[test]
    fn hold_pulses_at_lists_held_doors_and_skips_the_rest() {
        let now = Utc::now();
        let refresh = chrono::Duration::seconds(30);
        let held = Uuid::new_v4();
        let idle = Uuid::new_v4();
        let disabled = Uuid::new_v4();

        let snapshot = DoorStateSnapshot {
            snapshot_at: now,
            doors: vec![
                CompiledDoor {
                    id: held,
                    name: "Held".into(),
                    enabled: true,
                    unlock_duration_ms: 4200,
                    allow_cards: vec![],
                    deny_cards: vec![],
                    hold_unlock_until: Some(now + chrono::Duration::hours(1)),
                },
                CompiledDoor {
                    id: idle,
                    name: "Idle".into(),
                    enabled: true,
                    unlock_duration_ms: 4200,
                    allow_cards: vec!["A1".into()],
                    deny_cards: vec![],
                    hold_unlock_until: None,
                },
                // Held window but disabled -> must not be driven open.
                CompiledDoor {
                    id: disabled,
                    name: "Disabled".into(),
                    enabled: false,
                    unlock_duration_ms: 4200,
                    allow_cards: vec![],
                    deny_cards: vec![],
                    hold_unlock_until: Some(now + chrono::Duration::hours(1)),
                },
            ],
        };
        let s = DoorsState::default();
        s.apply_snapshot(snapshot);

        let pulses = s.hold_pulses_at(now, refresh);
        assert_eq!(pulses.len(), 1, "only the enabled, in-window door is held");
        assert_eq!(pulses[0].0, held);
        assert_eq!(pulses[0].1, 45_000);
    }

    #[test]
    fn open_access_still_honors_an_explicit_card_ban() {
        // An explicit deny beats the held-open window: a banned card stays out
        // even during public hours.
        let s = DoorsState::default();
        let id = Uuid::new_v4();
        let now = Utc::now();
        s.apply_snapshot(snap_held(
            id,
            &[],
            &["BANNED"],
            Some(now + chrono::Duration::hours(1)),
        ));
        assert!(matches!(s.decide_at(id, "BANNED", now), Decision::Deny(_)));
        // ...while a different card enters freely.
        assert!(matches!(
            s.decide_at(id, "GUEST", now),
            Decision::Allow { .. }
        ));
    }
}
