//! Door access — the wire types, and the Open Access hold arithmetic.
//!
//! The server compiles each device's per-door allow/deny lists and publishes
//! them to `{namespace}/devices/{device_id}/doors/state`. Since #191 that is
//! an alias of `resource/policy`, and the edge decides RFID scans from the one
//! cache in [`crate::resources`] -- fast and resilient to server outages --
//! rather than from a door cache of its own. What remains here is the shape
//! of the alias, the shapes of the door-only messages, and the hold.
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
//! re-sending that pulse faster than it elapses — see [`hold_pulse_ms`] and
//! [`crate::resources::ResourceCache::tick`]. It is driven off this
//! device's own clock and simply stops at the window end, so the strike
//! auto-relocks on its own if a window closes, the edge dies, or the server
//! link drops (fail-secure + edge-local expiry).

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
    /// #120 (#121): HMAC of the command, present when the server holds this
    /// device's command key. `#[serde(default)]` keeps a keyless/legacy server's
    /// unsigned command parseable.
    #[serde(default)]
    pub sig: Option<String>,
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
    /// #120 (#121): HMAC of the event so the server can reject a forged
    /// doors/event. Set by the edge when it holds a command key; omitted
    /// otherwise (a keyless server accepts it, rollout-graceful).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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
}
