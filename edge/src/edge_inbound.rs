//! Transport-agnostic dispatcher for server → edge messages. Used by both
//! the MQTT subscriber and the WebSocket reader so the per-message handling
//! lives in one place.
//!
//! Message vocabulary is the same `kind` strings defined in
//! [`css_lib::wire::kinds`] — these line up 1:1 with the MQTT topic
//! suffixes the server already publishes.

use std::sync::{Arc, RwLock};

use anyhow::Result;
use css_lib::wire::kinds;
use serde::Deserialize;
use tracing::{info, warn};

use crate::config::Config;
use crate::doors::{DoorStateSnapshot, DoorsState, UnlockCommand};
use crate::mqtt::DoorsUnlockSender;
use crate::toolguard::ToolGuardState;

#[derive(Deserialize)]
struct NameUpdate {
    name: String,
}

#[derive(Clone)]
pub struct EdgeInbound {
    pub config_manager: Arc<RwLock<Config>>,
    pub toolguard_state: Arc<ToolGuardState>,
    pub doors_state: Arc<DoorsState>,
    pub doors_unlock_tx: DoorsUnlockSender,
}

impl EdgeInbound {
    pub async fn dispatch(&self, kind: &str, payload: &[u8]) {
        match kind {
            kinds::NAME => match serde_json::from_slice::<NameUpdate>(payload) {
                Ok(msg) => {
                    info!("Received name update: {}", msg.name);
                    if let Err(e) = self.update_device_name(&msg.name) {
                        warn!("Failed to apply name update: {}", e);
                    }
                }
                Err(e) => warn!("Failed to parse name update: {}", e),
            },
            kinds::TOOLGUARD_STATE => match self.toolguard_state.apply_sync_bytes(payload) {
                Ok(()) => info!("ToolGuard state updated via inbound transport"),
                Err(e) => warn!("Failed to parse toolguard state payload: {}", e),
            },
            kinds::DOORS_STATE => match serde_json::from_slice::<DoorStateSnapshot>(payload) {
                Ok(snapshot) => {
                    let count = snapshot.doors.len();
                    self.doors_state.apply_snapshot(snapshot);
                    info!("Doors state updated via inbound transport ({} door(s))", count);
                }
                Err(e) => warn!("Failed to parse doors/state payload: {}", e),
            },
            kinds::DOORS_UNLOCK => match serde_json::from_slice::<UnlockCommand>(payload) {
                Ok(cmd) => {
                    info!(
                        "Received doors/unlock for door {} (reason={})",
                        cmd.door_id, cmd.reason
                    );
                    if self.doors_unlock_tx.send(cmd).is_err() {
                        warn!("Doors unlock bridge channel closed; cannot forward");
                    }
                }
                Err(e) => warn!("Failed to parse doors/unlock payload: {}", e),
            },
            other => warn!("EdgeInbound: ignoring unknown kind '{}'", other),
        }
    }

    fn update_device_name(&self, new_name: &str) -> Result<()> {
        let mut config = self
            .config_manager
            .write()
            .map_err(|_| anyhow::anyhow!("config lock poisoned"))?;
        config.name = new_name.to_string();
        if let Some(path) = std::env::var_os("CONFIG_PATH") {
            config.to_file(path)?;
        }
        Ok(())
    }
}
