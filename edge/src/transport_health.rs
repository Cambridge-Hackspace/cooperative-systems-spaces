//! What the edge knows about its own link to the server, so that "offline" is
//! answerable locally instead of only from the portal.
//!
//! #207 was invisible from the edge: its WebSocket task panicked at startup on
//! every boot, and because HTTP sync, the local broker and scan answers all
//! kept working, the only symptom was the portal showing the device offline.
//! `GET /api/status` reported `is_registered` and `auth_status` and had nothing
//! to say about whether the remote transport was actually up, so the operator
//! standing at the device could not tell a broken link from a working one.
//!
//! It also carries the negotiated key exchange group, which is the only honest
//! way to know whether a connection is post-quantum. The edge *prefers* hybrid
//! X25519+ML-KEM (rustls's `prefer-post-quantum` with the aws-lc-rs provider),
//! but preference is not outcome: whatever terminates TLS in front of
//! css-server is a reverse proxy outside this repository -- css-server serves
//! plain HTTP -- and if it does not offer ML-KEM the handshake falls back to
//! classical X25519 with nothing said. Reporting the group turns that from an
//! assumption into a reading.
//!
//! Injected from `main` rather than reached through a global, because the web
//! server is started before the transport exists and the two want different
//! lifetimes. The `Option`s are honest about that ordering: before a transport
//! has declared itself there is genuinely nothing to report.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

use serde::Serialize;

#[derive(Debug, Default)]
pub struct TransportHealth {
    kind: RwLock<Option<&'static str>>,
    /// Whether the active transport actually reports its own liveness. The
    /// WebSocket client does, because it owns its connection lifetime. The MQTT
    /// client does not: paho reconnects internally and the edge never sees the
    /// transitions. Without this flag the status endpoint would have to choose
    /// between two lies for MQTT -- `connected: false` on a working link, or
    /// `connected: true` on a dead one. `None` is the true answer.
    tracks_liveness: AtomicBool,
    connected: AtomicBool,
    key_exchange: RwLock<Option<String>>,
    /// How many times the transport's supervisor task has had to be restarted.
    /// Non-zero means something killed it -- a panic, most likely -- and that is
    /// worth seeing even once the link is back up, because the old behaviour was
    /// for exactly that to happen once at boot and never be mentioned again.
    supervisor_restarts: AtomicU64,
}

impl TransportHealth {
    pub fn new() -> Self {
        Self::default()
    }

    /// Name the transport in use, and promise to report its connection state.
    /// Called once, by a client that owns its connection lifetime.
    pub fn declare_tracked(&self, kind: &'static str) {
        *self.kind.write().unwrap() = Some(kind);
        self.tracks_liveness.store(true, Ordering::Release);
    }

    /// Name the transport in use without claiming to know whether it is up.
    pub fn declare_untracked(&self, kind: &'static str) {
        *self.kind.write().unwrap() = Some(kind);
        self.tracks_liveness.store(false, Ordering::Release);
    }

    /// A link is up. `key_exchange` is the negotiated group where the transport
    /// can see one, and `None` where it cannot -- MQTT over TLS through paho
    /// does not expose it, and saying nothing is better than implying classical.
    pub fn mark_connected(&self, key_exchange: Option<String>) {
        *self.key_exchange.write().unwrap() = key_exchange;
        self.connected.store(true, Ordering::Release);
    }

    pub fn mark_disconnected(&self) {
        self.connected.store(false, Ordering::Release);
        *self.key_exchange.write().unwrap() = None;
    }

    /// Returns the new total, so the caller can log it.
    pub fn note_supervisor_restart(&self) -> u64 {
        self.connected.store(false, Ordering::Release);
        self.supervisor_restarts.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn snapshot(&self) -> TransportStatus {
        TransportStatus {
            kind: *self.kind.read().unwrap(),
            connected: self
                .tracks_liveness
                .load(Ordering::Acquire)
                .then(|| self.connected.load(Ordering::Acquire)),
            key_exchange: self.key_exchange.read().unwrap().clone(),
            supervisor_restarts: self.supervisor_restarts.load(Ordering::Acquire),
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TransportStatus {
    /// `"websocket"`, `"mqtt"`, or `None` before a transport has started.
    pub kind: Option<&'static str>,
    /// `None` when the active transport does not report its own liveness; see
    /// `tracks_liveness`. Never a guess.
    pub connected: Option<bool>,
    /// e.g. `"X25519MLKEM768"` for a hybrid post-quantum handshake, or
    /// `"X25519"` for a classical one. `None` when the transport cannot report.
    pub key_exchange: Option<String>,
    pub supervisor_restarts: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_health_object_claims_nothing() {
        // The pre-transport state has to be distinguishable from "connected",
        // or the status endpoint would report a healthy link before one exists.
        let h = TransportHealth::new();
        let s = h.snapshot();
        assert_eq!(s.kind, None);
        assert_eq!(
            s.connected, None,
            "no transport is not the same as a dead one"
        );
        assert_eq!(s.key_exchange, None);
        assert_eq!(s.supervisor_restarts, 0);
    }

    #[test]
    fn an_untracked_transport_is_named_but_not_judged() {
        // The MQTT case. Reporting `connected: false` here would be the same
        // kind of falsehood #207 produced from the other direction: a reading
        // that looks like a measurement and is not one.
        let h = TransportHealth::new();
        h.declare_untracked("mqtt");
        let s = h.snapshot();
        assert_eq!(s.kind, Some("mqtt"));
        assert_eq!(s.connected, None, "untracked must not read as down");
    }

    #[test]
    fn disconnecting_forgets_the_negotiated_group() {
        // A stale key_exchange on a dead link would be the same class of lie as
        // #207's silence: a reading that looks like present-tense fact.
        let h = TransportHealth::new();
        h.declare_tracked("websocket");
        h.mark_connected(Some("X25519MLKEM768".into()));
        assert_eq!(h.snapshot().key_exchange.as_deref(), Some("X25519MLKEM768"));
        h.mark_disconnected();
        let s = h.snapshot();
        assert_eq!(s.connected, Some(false));
        assert_eq!(s.key_exchange, None, "a dead link must not report a group");
        assert_eq!(
            s.kind,
            Some("websocket"),
            "the transport in use is still known"
        );
    }

    #[test]
    fn a_restart_is_counted_and_drops_the_connection() {
        let h = TransportHealth::new();
        h.declare_tracked("websocket");
        h.mark_connected(Some("X25519".into()));
        assert_eq!(h.note_supervisor_restart(), 1);
        assert_eq!(h.note_supervisor_restart(), 2);
        let s = h.snapshot();
        assert_eq!(s.supervisor_restarts, 2);
        assert_eq!(
            s.connected,
            Some(false),
            "a supervisor that died is not a live link"
        );
    }
}
