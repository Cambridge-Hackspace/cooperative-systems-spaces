//! Outbound transport routing for server → device messages.
//!
//! Every server-initiated push (door unlock, doors state snapshot, toolguard
//! state, name change) goes through [`DeviceTransport::push`]. The transport
//! prefers an active WebSocket session for the device and falls back to MQTT
//! if no WS session is registered. This lets a deployer run the system
//! without an MQTT broker entirely (WS only) or without WS (MQTT only).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use css_lib::wire::WireMessage;
use tokio::sync::mpsc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::mqtt::MqttService;

/// Capacity of a per-device WebSocket send queue. #120 (#15): the queue used to
/// be unbounded, so a device that connected but stopped draining would grow it
/// without limit. A full bounded queue makes `try_send` fail and `push` falls
/// back to MQTT (or reports the message undelivered) rather than buffering
/// forever.
pub const DEVICE_CHANNEL_CAPACITY: usize = 256;

/// One connection's outbound sender, tagged with the epoch that identifies it.
struct Session {
    /// Monotonic id of this specific connection. A later connection gets a
    /// higher epoch; an older connection's teardown must not evict a newer
    /// session's sender.
    epoch: u64,
    tx: mpsc::Sender<WireMessage>,
}

/// One slot per WebSocket-connected device. When a device disconnects the
/// session removes itself from the registry so subsequent pushes fall back
/// to MQTT (or warn if MQTT isn't configured).
#[derive(Default)]
pub struct DeviceChannelRegistry {
    inner: RwLock<HashMap<Uuid, Session>>,
    next_epoch: AtomicU64,
}

impl DeviceChannelRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register a connection's sender, returning its epoch. #120 (#15): pass the
    /// returned epoch back to [`unregister`](Self::unregister) so a stale
    /// connection's cleanup cannot evict the sender a newer reconnect installed.
    /// Before this, `unregister(device_id)` removed whatever was in the slot --
    /// so a reconnect (which overwrote the slot) was silently unregistered when
    /// the *old* socket finally closed, and the live device went unreachable.
    pub fn register(&self, device_id: Uuid, tx: mpsc::Sender<WireMessage>) -> u64 {
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        let mut map = self.inner.write().expect("device registry poisoned");
        map.insert(device_id, Session { epoch, tx });
        epoch
    }

    /// Remove a connection's sender, but only if it is still the current one for
    /// this device (its epoch matches). A mismatch means a newer connection has
    /// taken the slot and must be left in place.
    pub fn unregister(&self, device_id: Uuid, epoch: u64) {
        let mut map = self.inner.write().expect("device registry poisoned");
        if map.get(&device_id).is_some_and(|s| s.epoch == epoch) {
            map.remove(&device_id);
        }
    }

    pub fn contains(&self, device_id: Uuid) -> bool {
        self.inner
            .read()
            .map(|m| m.contains_key(&device_id))
            .unwrap_or(false)
    }

    /// Returns true if the message was queued on a WS session.
    pub fn try_send(&self, device_id: Uuid, msg: WireMessage) -> bool {
        let map = self.inner.read().expect("device registry poisoned");
        if let Some(s) = map.get(&device_id) {
            if s.tx.try_send(msg).is_ok() {
                return true;
            }
        }
        false
    }

    pub fn connected_count(&self) -> usize {
        self.inner.read().map(|m| m.len()).unwrap_or(0)
    }
}

/// Transport abstraction shared by every server → device publish site.
#[derive(Clone)]
pub struct DeviceTransport {
    registry: Arc<DeviceChannelRegistry>,
    mqtt: Option<Arc<MqttService>>,
}

impl DeviceTransport {
    pub fn new(registry: Arc<DeviceChannelRegistry>, mqtt: Option<Arc<MqttService>>) -> Self {
        Self { registry, mqtt }
    }

    pub fn registry(&self) -> &Arc<DeviceChannelRegistry> {
        &self.registry
    }

    /// Send `payload` to `device_id` under topic-suffix `kind`. Tries WS
    /// first, then MQTT. Returns `true` if the message was delivered to
    /// _some_ transport, `false` if neither was available.
    pub fn push(&self, device_id: Uuid, kind: &str, payload: serde_json::Value) -> bool {
        // Try WebSocket first.
        let msg = WireMessage::new(kind, payload.clone());
        if self.registry.try_send(device_id, msg) {
            debug!("Pushed '{}' to {} via WebSocket", kind, device_id);
            return true;
        }

        // Fall back to MQTT.
        if let Some(mqtt) = &self.mqtt {
            let bytes = match serde_json::to_vec(&payload) {
                Ok(b) => b,
                Err(e) => {
                    warn!("Failed to serialize payload for MQTT publish: {}", e);
                    return false;
                }
            };
            if let Err(e) = mqtt.publish_to_device(device_id, kind, bytes) {
                warn!(
                    "MQTT publish to {} (suffix '{}') failed: {}",
                    device_id, kind, e
                );
                return false;
            }
            debug!("Pushed '{}' to {} via MQTT", kind, device_id);
            return true;
        }

        warn!(
            "No transport available for device {} (kind '{}')",
            device_id, kind
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_round_trip() {
        let r = DeviceChannelRegistry::new();
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(4);
        let epoch = r.register(id, tx);
        assert!(r.contains(id));
        assert!(r.try_send(id, WireMessage::new("data", json!({"k": 1}))));
        let got = rx.try_recv().unwrap();
        assert_eq!(got.kind, "data");
        r.unregister(id, epoch);
        assert!(!r.contains(id));
    }

    // #120 (#15): a reconnect overwrites the slot; when the *old* connection
    // then tears down it must not evict the new sender. Under the old
    // epoch-less unregister this removed the reconnect's sender and the live
    // device fell silent (pushes dropped to MQTT-or-nothing).
    #[test]
    fn a_stale_connection_teardown_does_not_evict_a_reconnect() {
        let r = DeviceChannelRegistry::new();
        let id = Uuid::new_v4();
        let (tx_old, _rx_old) = mpsc::channel(4);
        let (tx_new, mut rx_new) = mpsc::channel(4);

        let epoch_old = r.register(id, tx_old);
        let epoch_new = r.register(id, tx_new); // reconnect takes the slot
        assert_ne!(epoch_old, epoch_new);

        // The OLD connection tears down last, referencing its own epoch.
        r.unregister(id, epoch_old);

        assert!(r.contains(id), "the reconnect's sender was wrongly evicted");
        assert!(r.try_send(id, WireMessage::new("data", json!({"k": 1}))));
        assert_eq!(rx_new.try_recv().unwrap().kind, "data");

        // The current connection can still remove itself.
        r.unregister(id, epoch_new);
        assert!(!r.contains(id));
    }

    #[test]
    fn push_uses_ws_when_present() {
        let r = DeviceChannelRegistry::new();
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(4);
        r.register(id, tx);
        let t = DeviceTransport::new(r, None);
        assert!(t.push(id, "name", json!({"name": "x"})));
        let got = rx.try_recv().unwrap();
        assert_eq!(got.kind, "name");
    }

    #[test]
    fn push_returns_false_when_nothing_available() {
        let t = DeviceTransport::new(DeviceChannelRegistry::new(), None);
        assert!(!t.push(Uuid::new_v4(), "name", json!({})));
    }
}
