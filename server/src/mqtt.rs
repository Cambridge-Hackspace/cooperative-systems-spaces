use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::MqttConfig;

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
    namespace: String,
    /// Transport-agnostic inbound dispatcher. Shared with the WebSocket path
    /// so the per-message handling lives in exactly one place.
    inbound: Arc<crate::devices_inbound::DeviceInbound>,
}

impl MqttService {
    /// Create a new MQTT service and consumer
    /// The database handle is deliberately absent from the signature.
    ///
    /// This service used to hold an `Arc<DatabaseManager>` and never read it:
    /// every message it receives goes to `DeviceInbound`, which owns the
    /// per-message handling so that the MQTT and WebSocket paths cannot drift
    /// apart. A field nobody reads on a service that plainly *could* need one is
    /// an invitation for the next person to reach for it here rather than there,
    /// and the two transports would then handle messages differently.
    pub fn new(
        config: &MqttConfig,
        inbound: Arc<crate::devices_inbound::DeviceInbound>,
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

        info!(
            "Connected to MQTT broker at {} with namespace: {}",
            broker_url, config.mqtt_namespace
        );

        Ok((
            Self {
                client: cli,
                namespace: config.mqtt_namespace.clone(),
                inbound,
            },
            rx,
        ))
    }

    /// Start the MQTT service and listen for messages
    pub async fn start(
        self,
        rx: mqtt::Receiver<Option<mqtt::Message>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
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

        // Suffixes are identical to `WireMessage::kind` strings — let the
        // shared inbound dispatcher do the actual work.
        self.inbound
            .dispatch(device_id, suffix, msg.payload())
            .await;
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
        self.publish_to_device(device_id, "name", payload.to_string().into_bytes())
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
