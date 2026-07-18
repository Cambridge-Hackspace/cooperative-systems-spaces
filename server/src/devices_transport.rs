//! Outbound transport routing for server → device messages.
//!
//! Every server-initiated push (door unlock, doors state snapshot, toolguard
//! state, name change) goes through [`DeviceTransport::push`]. The transport
//! prefers an active WebSocket session for the device and falls back to MQTT
//! if no WS session is registered. This lets a deployer run the system
//! without an MQTT broker entirely (WS only) or without WS (MQTT only).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use css_lib::wire::WireMessage;
use tokio::sync::mpsc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::mqtt::MqttService;

/// One slot per WebSocket-connected device. When a device disconnects the
/// session removes itself from the registry so subsequent pushes fall back
/// to MQTT (or warn if MQTT isn't configured).
#[derive(Default)]
pub struct DeviceChannelRegistry {
    inner: RwLock<HashMap<Uuid, mpsc::UnboundedSender<WireMessage>>>,
}

impl DeviceChannelRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn register(&self, device_id: Uuid, tx: mpsc::UnboundedSender<WireMessage>) {
        let mut map = self.inner.write().expect("device registry poisoned");
        map.insert(device_id, tx);
    }

    pub fn unregister(&self, device_id: Uuid) {
        let mut map = self.inner.write().expect("device registry poisoned");
        map.remove(&device_id);
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
        if let Some(tx) = map.get(&device_id) {
            if tx.send(msg).is_ok() {
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
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn registry_round_trip() {
        let r = DeviceChannelRegistry::new();
        let id = Uuid::new_v4();
        let (tx, mut rx) = unbounded_channel();
        r.register(id, tx);
        assert!(r.contains(id));
        assert!(r.try_send(id, WireMessage::new("data", json!({"k": 1}))));
        let got = rx.try_recv().unwrap();
        assert_eq!(got.kind, "data");
        r.unregister(id);
        assert!(!r.contains(id));
    }

    #[test]
    fn push_uses_ws_when_present() {
        let r = DeviceChannelRegistry::new();
        let id = Uuid::new_v4();
        let (tx, mut rx) = unbounded_channel();
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
