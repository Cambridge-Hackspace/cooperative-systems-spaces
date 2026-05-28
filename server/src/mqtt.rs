use chrono::Utc;
use diesel::prelude::*;
use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::MqttConfig;
use crate::database::DatabaseManager;
use crate::models::{AuditEventType, NewAuditLog, SpaceDevicePlatform, UpdateSpaceDevice};
use crate::schema::space_devices;

/// Device data payload sent by devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDataPayload {
    pub mac_address: String,
    pub software_version: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub uptime: i64,
    pub platform: String, // "windows", "linux", "macos", "other"
}

/// MQTT service for handling device communication
#[derive(Clone)]
pub struct MqttService {
    client: mqtt::AsyncClient,
    db: Arc<DatabaseManager>,
    namespace: String,
    /// Snapshot of `toolguard.profile_field` taken at startup; used by the
    /// inbound `doors/event` handler to resolve a card to a user.
    door_profile_field: String,
}

impl MqttService {
    /// Create a new MQTT service and consumer
    pub fn new(
        config: &MqttConfig,
        db: Arc<DatabaseManager>,
        door_profile_field: String,
    ) -> Result<(Self, mqtt::Receiver<Option<mqtt::Message>>), Box<dyn std::error::Error>> {
        // Parse broker URL
        let broker_url = &config.mqtt_instance_url;
        
        // Create MQTT options
        let create_opts = mqtt::CreateOptionsBuilder::new()
            .server_uri(broker_url)
            .client_id("css-server")
            .finalize();
        
        // Create the client
        let cli = mqtt::AsyncClient::new(create_opts)?;
        
        // Get the receiver before connecting
        let rx = cli.start_consuming();
        
        // Build connection options
        let mut conn_opts_builder = mqtt::ConnectOptionsBuilder::new();
        conn_opts_builder
            .keep_alive_interval(Duration::from_secs(30))
            .clean_session(true)
            .automatic_reconnect(Duration::from_secs(1), Duration::from_secs(30));
        
        // Set credentials if provided
        if let (Some(username), Some(password)) = (&config.mqtt_username, &config.mqtt_password) {
            conn_opts_builder.user_name(username).password(password);
        }
        
        let conn_opts = conn_opts_builder.finalize();
        
        // Connect to the broker
        cli.connect(conn_opts).wait()?;
        
        info!("Connected to MQTT broker at {} with namespace: {}", broker_url, config.mqtt_namespace);
        
        Ok((Self {
            client: cli,
            db,
            namespace: config.mqtt_namespace.clone(),
            door_profile_field,
        }, rx))
    }

    /// Start the MQTT service and listen for messages
    pub async fn start(self, rx: mqtt::Receiver<Option<mqtt::Message>>) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting MQTT service with namespace: {}", self.namespace);

        // Subscribe to device topics with namespace prefix
        let heartbeat_topic = format!("{}/devices/+/heartbeat", self.namespace);
        let data_topic = format!("{}/devices/+/data", self.namespace);
        let doors_event_topic = format!("{}/devices/+/doors/event", self.namespace);

        self.client.subscribe(&heartbeat_topic, 1).wait()?;
        self.client.subscribe(&data_topic, 1).wait()?;
        self.client.subscribe(&doors_event_topic, 1).wait()?;

        info!(
            "Subscribed to device topics: {}, {}, {}",
            heartbeat_topic, data_topic, doors_event_topic
        );

        // Process incoming messages
        loop {
            match rx.recv() {
                Ok(Some(msg)) => {
                    self.handle_message(msg).await;
                }
                Ok(None) => {
                    // Connection lost, try to reconnect
                    warn!("MQTT connection lost, waiting for reconnection...");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    error!("Error receiving message: {}", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Handle incoming MQTT messages
    async fn handle_message(&self, msg: mqtt::Message) {
        let topic = msg.topic();

        // Topic format: {namespace}/devices/{device_id}/{message_type}
        // Check if topic starts with the namespace
        let expected_prefix = format!("{}/devices/", self.namespace);
        if !topic.starts_with(&expected_prefix) {
            warn!("Topic does not match expected namespace prefix: {}", topic);
            return;
        }

        // Strip the namespace and "devices/" prefix. Remaining is
        // "{device_id}/{suffix}" where suffix may contain '/'.
        let remaining = &topic[expected_prefix.len()..];
        let (device_id_str, suffix) = match remaining.split_once('/') {
            Some(parts) => parts,
            None => {
                warn!("Invalid topic format after namespace: {}", topic);
                return;
            }
        };

        let device_id = match Uuid::parse_str(device_id_str) {
            Ok(id) => id,
            Err(e) => {
                warn!("Invalid device ID in topic {}: {}", topic, e);
                return;
            }
        };

        match suffix {
            "heartbeat" => self.handle_heartbeat(device_id).await,
            "data" => self.handle_device_data(device_id, msg.payload()).await,
            "doors/event" => self.handle_doors_event(device_id, msg.payload()).await,
            other => {
                warn!("Unknown message suffix '{}' on topic {}", other, topic);
            }
        }
    }

    /// Handle heartbeat messages
    async fn handle_heartbeat(&self, device_id: Uuid) {
        info!("Received heartbeat from device: {}", device_id);

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
            Ok(_) => {
                warn!("Device not found or already deleted: {}", device_id);
            }
            Err(e) => {
                error!("Failed to update device last_seen_at: {}", e);
            }
        }
    }

    /// Handle device data update messages
    async fn handle_device_data(&self, device_id: Uuid, payload: &[u8]) {
        info!("Received data update from device: {}", device_id);

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

        // Parse platform
        let platform = match data.platform.to_lowercase().as_str() {
            "windows" => SpaceDevicePlatform::Windows,
            "linux" => SpaceDevicePlatform::Linux,
            "macos" => SpaceDevicePlatform::MacOs,
            _ => SpaceDevicePlatform::Other,
        };

        // Get old version for audit logging
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

                // Create audit log if version changed
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
            Ok(_) => {
                warn!("Device not found or already deleted: {}", device_id);
            }
            Err(e) => {
                error!("Failed to update device data: {}", e);
            }
        }
    }

    /// Handle a `doors/event` message published by an edge device. Writes a
    /// row to `door_access_events`, resolves the card to a user via the
    /// configured toolguard profile field, and emits an audit event so
    /// webhook subscribers see it.
    async fn handle_doors_event(&self, device_id: Uuid, payload: &[u8]) {
        #[derive(serde::Deserialize)]
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
                error!("Failed to parse doors/event payload from device {}: {}", device_id, e);
                return;
            }
        };

        // Resolve card -> user. Misses are normal (unknown card).
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
            Some("rfid") | None => crate::models::DoorAccessMethod::Rfid,
            Some("admin_remote") => crate::models::DoorAccessMethod::AdminRemote,
            Some("qr_checkin") => crate::models::DoorAccessMethod::QrCheckin,
            Some(other) => {
                warn!("Unknown doors/event source '{}' from device {}", other, device_id);
                crate::models::DoorAccessMethod::Rfid
            }
        };

        let new_event = crate::models::NewDoorAccessEvent {
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

        // Audit event mirrors the access event so webhook subscribers receive it.
        let audit_type = if event.granted {
            crate::models::AuditEventType::DoorUnlockedCard
        } else {
            crate::models::AuditEventType::DoorUnlockDenied
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

    /// Publish a message to a device topic
    pub fn publish_to_device(
        &self,
        device_id: Uuid,
        topic_suffix: &str,
        payload: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let topic = format!("{}/devices/{}/{}", self.namespace, device_id, topic_suffix);
        let msg = mqtt::Message::new(topic, payload, 1);
        self.client.publish(msg).wait()?;
        Ok(())
    }

    /// Publish a name change command to a device
    pub fn publish_name_change(
        &self,
        device_id: Uuid,
        new_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let payload = serde_json::json!({
            "name": new_name
        });
        self.publish_to_device(
            device_id,
            "name",
            payload.to_string().into_bytes(),
        )
    }

    /// Publish a ToolGuard state update to a specific device
    pub fn publish_toolguard_state(
        &self,
        device_id: Uuid,
        payload: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_to_device(device_id, "toolguard/state", payload)
    }

    /// Publish a doors state snapshot (allow/deny lists) to a device.
    pub fn publish_doors_state(
        &self,
        device_id: Uuid,
        payload: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_to_device(device_id, "doors/state", payload)
    }

    /// Publish a one-shot door-unlock command to a device.
    pub fn publish_doors_unlock(
        &self,
        device_id: Uuid,
        payload: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.publish_to_device(device_id, "doors/unlock", payload)
    }

    /// Get a reference to the MQTT client for publishing
    pub fn client(&self) -> &mqtt::AsyncClient {
        &self.client
    }
}
