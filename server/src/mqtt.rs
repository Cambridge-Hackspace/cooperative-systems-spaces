use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::MqttConfig;

/// Depth of the inbound message stream.
///
/// Bounded on purpose. An unbounded stream turns a slow or stuck consumer into
/// unbounded memory growth; with a bound, a burst that outruns us is dropped by
/// paho and logged by the broker rather than taking the process down. 100 is
/// comfortably above the steady-state rate here (device heartbeats and door
/// events), so it only bites during a genuine flood.
const STREAM_BUFFER: usize = 100;

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
    pub async fn new(
        config: &MqttConfig,
        inbound: Arc<crate::devices_inbound::DeviceInbound>,
    ) -> Result<(Self, mqtt::AsyncReceiver<Option<mqtt::Message>>), Box<dyn std::error::Error>>
    {
        // Parse broker URL
        let broker_url = &config.mqtt_instance_url;

        // Create MQTT options
        let create_opts = mqtt::CreateOptionsBuilder::new()
            .server_uri(broker_url)
            .client_id("css-server")
            .finalize();

        // Create the client
        let mut cli = mqtt::AsyncClient::new(create_opts)?;

        // Get the receiver before connecting.
        //
        // `get_stream` and NOT `start_consuming`. The latter returns a
        // synchronous channel whose `recv()` parks the OS thread it is called
        // on. Doing that from a tokio task starves the runtime: a blocked
        // thread cannot be preempted and its work cannot be stolen, so on a
        // small host (one worker per core) losing the broker stopped the HTTP
        // server accepting connections entirely. See `start` below.
        let rx = cli.get_stream(STREAM_BUFFER);

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
        cli.connect(conn_opts).await?;

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

    /// Subscribe to the device topics this service consumes.
    ///
    /// Split out because it has to run twice: once at startup, and again after
    /// every reconnect. The connection is made with `clean_session(true)`, so
    /// the broker discards our subscriptions when the link drops -- a
    /// reconnected client that does not re-subscribe is connected and deaf,
    /// which looks healthy from the outside and silently stops delivering
    /// device heartbeats, data and door events.
    async fn subscribe_all(&self) -> Result<(), mqtt::Error> {
        let heartbeat_topic = format!("{}/devices/+/heartbeat", self.namespace);
        let data_topic = format!("{}/devices/+/data", self.namespace);
        let doors_event_topic = format!("{}/devices/+/doors/event", self.namespace);

        self.client.subscribe(&heartbeat_topic, 1).await?;
        self.client.subscribe(&data_topic, 1).await?;
        self.client.subscribe(&doors_event_topic, 1).await?;

        info!(
            "Subscribed to device topics: {}, {}, {}",
            heartbeat_topic, data_topic, doors_event_topic
        );
        Ok(())
    }

    /// Start the MQTT service and listen for messages.
    ///
    /// Every wait in here is an `.await`, never a blocking call. This task runs
    /// on the same tokio runtime as the HTTP server, so a blocking receive
    /// parks a worker thread that cannot then be preempted or work-stolen --
    /// which is exactly how a broker outage used to take the whole web server
    /// down with it while the process stayed up and the port stayed open.
    pub async fn start(
        self,
        rx: mqtt::AsyncReceiver<Option<mqtt::Message>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting MQTT service with namespace: {}", self.namespace);

        self.subscribe_all().await?;

        // Process incoming messages
        loop {
            match rx.recv().await {
                Ok(Some(msg)) => {
                    self.handle_message(msg).await;
                }
                Ok(None) => {
                    // A `None` item is paho signalling that the connection
                    // dropped. `automatic_reconnect` re-establishes it for us;
                    // what it cannot do is restore subscriptions, so wait for
                    // the link to come back and then re-subscribe ourselves.
                    warn!("MQTT connection lost, waiting for reconnection...");
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if !self.client.is_connected() {
                            continue;
                        }
                        match self.subscribe_all().await {
                            Ok(()) => {
                                info!("MQTT reconnected, subscriptions restored");
                                break;
                            }
                            Err(e) => {
                                warn!("MQTT reconnected but re-subscribe failed: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    // The stream is closed: paho dropped the sending half, so
                    // no further message can ever arrive and retrying the
                    // receive would spin on an error that cannot change. (The
                    // old code slept a second and retried, which turned this
                    // into a 1 Hz error log for the life of the process.)
                    //
                    // Reported as an error, not a clean exit: a consumer that
                    // has stopped consuming is a failure, and returning Ok here
                    // would tell `main` the task finished its job. Devices
                    // would go unheard with nothing above this saying so.
                    error!("MQTT stream closed, stopping consumer: {}", e);
                    return Err(Box::new(e));
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

    /// Publish a message to a device topic.
    ///
    /// Deliberately does not block on delivery. This is reached from
    /// `DeviceTransport::push`, which is synchronous and called from request
    /// handlers and the door schedule ticker; `.wait()`-ing here parked a
    /// runtime worker until the broker ACKed, which on a half-open connection
    /// means up to the keep-alive interval. Same class of defect as the receive
    /// loop this file used to have.
    ///
    /// The error this returns is therefore "could not hand the message over"
    /// (no connection), not "the device did not receive it". That matches what
    /// the one caller actually does with it -- `push` uses it to decide whether
    /// MQTT was a usable transport at all, and falls back accordingly. Delivery
    /// failures after hand-off are logged from the spawned task instead, which
    /// is where they were always really observed: the old code only learned
    /// about them after blocking for the timeout anyway.
    pub fn publish_to_device(
        &self,
        device_id: Uuid,
        topic_suffix: &str,
        payload: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.client.is_connected() {
            return Err("MQTT client is not connected".into());
        }

        let topic = format!("{}/devices/{}/{}", self.namespace, device_id, topic_suffix);
        let msg = mqtt::Message::new(topic, payload, 1);
        let token = self.client.publish(msg);

        // Observe the outcome without blocking. `try_current` rather than
        // `tokio::spawn` because that panics off-runtime; every caller today is
        // on the runtime, but a publish is not worth a panic if one ever is not.
        // Dropping the token does not cancel the publish -- the message is
        // already queued in the client -- we just stop watching it.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if let Err(e) = token.await {
                        warn!("MQTT delivery to {} failed: {}", device_id, e);
                    }
                });
            }
            Err(_) => {
                debug!("No tokio runtime; not observing delivery to {}", device_id);
            }
        }
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
