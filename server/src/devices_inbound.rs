//! Transport-agnostic dispatcher for inbound device → server messages.
//!
//! Both the MQTT subscriber (`mqtt.rs`) and the WebSocket reader
//! (`api/devices.rs::device_ws`) call into this so the per-message handling
//! lives in exactly one place regardless of how the bytes arrived.

use std::sync::Arc;

use chrono::Utc;
use css_lib::wire::kinds;
use diesel::prelude::*;
use serde::Deserialize;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::database::DatabaseManager;
use crate::models::{
    AuditEventType, DoorAccessMethod, NewAuditLog, NewDoorAccessEvent, SpaceDevicePlatform,
    UpdateSpaceDevice,
};
use crate::schema::space_devices;

/// Shared by every transport that carries device telemetry.
#[derive(Clone)]
pub struct DeviceInbound {
    db: Arc<DatabaseManager>,
    /// Snapshot of `toolguard.profile_field` taken at startup; used by the
    /// doors/event handler to resolve a card to a user.
    door_profile_field: String,
}

impl DeviceInbound {
    pub fn new(db: Arc<DatabaseManager>, door_profile_field: String) -> Self {
        Self {
            db,
            door_profile_field,
        }
    }

    /// Route a message by `kind`. Unknown kinds are warned-and-dropped (we
    /// never want one bad device version to break the receive loop).
    pub async fn dispatch(&self, device_id: Uuid, kind: &str, payload: &[u8]) {
        match kind {
            kinds::HEARTBEAT => self.handle_heartbeat(device_id).await,
            kinds::DATA => self.handle_device_data(device_id, payload).await,
            kinds::DOORS_EVENT => self.handle_doors_event(device_id, payload).await,
            other => warn!(
                "DeviceInbound: ignoring unknown kind '{}' from device {}",
                other, device_id
            ),
        }
    }

    pub async fn handle_heartbeat(&self, device_id: Uuid) {
        let mut conn = match self.db.pool().get() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get DB connection: {}", e);
                return;
            }
        };

        let update = UpdateSpaceDevice {
            last_seen_at: Some(Utc::now()),
            ..Default::default()
        };

        match diesel::update(space_devices::table)
            .filter(space_devices::id.eq(device_id))
            .filter(space_devices::deleted_at.is_null())
            .set(&update)
            .execute(&mut conn)
        {
            Ok(rows) if rows > 0 => {
                tracing::debug!("Updated last_seen_at for device {}", device_id);
            }
            Ok(_) => warn!("Device not found or already deleted: {}", device_id),
            Err(e) => error!("Failed to update device last_seen_at: {}", e),
        }
    }

    pub async fn handle_device_data(&self, device_id: Uuid, payload: &[u8]) {
        #[derive(Deserialize)]
        struct DeviceDataPayload {
            mac_address: String,
            software_version: String,
            ipv4_address: Option<String>,
            ipv6_address: Option<String>,
            uptime: i64,
            platform: String,
        }

        let data: DeviceDataPayload = match serde_json::from_slice(payload) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to parse device data payload: {}", e);
                return;
            }
        };

        let mut conn = match self.db.pool().get() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get DB connection: {}", e);
                return;
            }
        };

        let platform = match data.platform.to_lowercase().as_str() {
            "windows" => SpaceDevicePlatform::Windows,
            "linux" => SpaceDevicePlatform::Linux,
            "macos" => SpaceDevicePlatform::MacOs,
            _ => SpaceDevicePlatform::Other,
        };

        let old_version: Option<String> = space_devices::table
            .filter(space_devices::id.eq(device_id))
            .select(space_devices::software_version)
            .first(&mut conn)
            .ok();

        let update = UpdateSpaceDevice {
            mac_address: Some(data.mac_address),
            software_version: Some(data.software_version.clone()),
            ipv4_address: data.ipv4_address.clone(),
            ipv6_address: data.ipv6_address.clone(),
            uptime: Some(data.uptime),
            platform: Some(platform),
            updated_at: Some(Utc::now()),
            ..Default::default()
        };

        match diesel::update(space_devices::table)
            .filter(space_devices::id.eq(device_id))
            .filter(space_devices::deleted_at.is_null())
            .set(&update)
            .execute(&mut conn)
        {
            Ok(rows) if rows > 0 => {
                info!(
                    "Updated device data for {}: version={}, uptime={}",
                    device_id, data.software_version, data.uptime
                );
                if let Some(old_ver) = old_version {
                    if old_ver != data.software_version {
                        let audit_log = NewAuditLog {
                            event_type: AuditEventType::DeviceVersionChanged.as_str().to_string(),
                            user_id: None,
                            actor_id: None,
                            event_data: serde_json::json!({
                                "device_id": device_id,
                                "old_version": old_ver,
                                "new_version": data.software_version,
                            }),
                            ip_address: None,
                            user_agent: None,
                        };
                        if let Err(e) = self.db.create_audit_log(&audit_log) {
                            error!("Failed to create audit log for version change: {}", e);
                        }
                    }
                }
            }
            Ok(_) => warn!("Device not found or already deleted: {}", device_id),
            Err(e) => error!("Failed to update device data: {}", e),
        }
    }

    pub async fn handle_doors_event(&self, device_id: Uuid, payload: &[u8]) {
        #[derive(Deserialize)]
        struct DoorEventIn {
            door_id: Uuid,
            card_id: Option<String>,
            granted: bool,
            #[serde(default)]
            reason: Option<String>,
            #[serde(default)]
            source: Option<String>,
            #[serde(default)]
            occurred_at: Option<chrono::DateTime<Utc>>,
        }

        let event: DoorEventIn = match serde_json::from_slice(payload) {
            Ok(e) => e,
            Err(e) => {
                error!(
                    "Failed to parse doors/event payload from device {}: {}",
                    device_id, e
                );
                return;
            }
        };

        let user_id = match event.card_id.as_deref() {
            Some(card) if !card.is_empty() => self
                .db
                .find_user_by_profile_field(&self.door_profile_field, card)
                .ok()
                .flatten()
                .map(|u| u.id),
            _ => None,
        };

        let method = match event.source.as_deref() {
            Some("rfid") | None => DoorAccessMethod::Rfid,
            Some("admin_remote") => DoorAccessMethod::AdminRemote,
            Some("qr_checkin") => DoorAccessMethod::QrCheckin,
            Some(other) => {
                warn!(
                    "Unknown doors/event source '{}' from device {}",
                    other, device_id
                );
                DoorAccessMethod::Rfid
            }
        };

        let new_event = NewDoorAccessEvent {
            door_id: event.door_id,
            user_id,
            method: method.as_str().to_string(),
            card_id_attempted: event.card_id.clone(),
            granted: event.granted,
            reason: event.reason.clone(),
            ip_address: None,
            occurred_at: event.occurred_at.unwrap_or_else(Utc::now),
        };

        if let Err(e) = self.db.insert_door_access_event(&new_event) {
            error!("Failed to insert door_access_events row: {}", e);
            return;
        }

        let audit_type = if event.granted {
            AuditEventType::DoorUnlockedCard
        } else {
            AuditEventType::DoorUnlockDenied
        };
        let audit_log = NewAuditLog {
            event_type: audit_type.as_str().to_string(),
            user_id,
            actor_id: user_id,
            event_data: serde_json::json!({
                "door_id": event.door_id,
                "device_id": device_id,
                "card_id_attempted": event.card_id,
                "granted": event.granted,
                "reason": event.reason,
                "source": method.as_str(),
            }),
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&audit_log) {
            error!("Failed to write door access audit log: {}", e);
        }
    }
}
