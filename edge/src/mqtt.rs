use anyhow::{Context, Result};
use paho_mqtt as mqtt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use css_lib::wire::{
    local, LocalAliveRequest, LocalPower, LocalScan, LocalScanReply, LocalStop,
    LocalToolLogRequest, LocalToolOnResponse, LocalUsage, ResourceLeasePayload,
};

use crate::config::{Config, MqttConfig};
use crate::doors::{self, DoorsEvent, LocalScanRequest, LocalUnlockResponse, UnlockCommand};
use crate::edge_inbound::EdgeInbound;
use crate::power::PowerState;
use crate::registration::{get_auth_token, get_device_id};
use crate::resources::{Kind, ResourceCache, ScanDecision};
use crate::system_info::get_system_info;
use crate::toolguard::ToolGuardState;

/// Cross-MQTT channels for the door bridge. The remote client forwards
/// `doors/unlock` commands here; the local client forwards local scan
/// decisions back out as `doors/event`. Tasks spawned in `main` drain them.
pub type DoorsUnlockSender = tokio::sync::mpsc::UnboundedSender<UnlockCommand>;
pub type DoorsUnlockReceiver = tokio::sync::mpsc::UnboundedReceiver<UnlockCommand>;
pub type DoorsEventSender = tokio::sync::mpsc::UnboundedSender<DoorsEvent>;
pub type DoorsEventReceiver = tokio::sync::mpsc::UnboundedReceiver<DoorsEvent>;

#[derive(Debug, Serialize)]
struct DeviceData {
    mac_address: String,
    software_version: String,
    ipv4_address: Option<String>,
    ipv6_address: Option<String>,
    uptime: u64,
    platform: String,
}

/// MQTT Client Manager for edge apparatus
pub struct EdgeMqttClient {
    client: mqtt::AsyncClient,
    device_id: String,
    namespace: String,
    start_time: std::time::Instant,
    /// Shared with the WebSocket transport; handles every server-pushed message.
    inbound: Arc<EdgeInbound>,
}

impl EdgeMqttClient {
    /// Create a new MQTT client
    pub async fn new(
        config: &Config,
        inbound: Arc<EdgeInbound>,
    ) -> Result<(Self, mqtt::Receiver<Option<mqtt::Message>>)> {
        let device_id = get_device_id(config)
            .ok_or_else(|| anyhow::anyhow!("Device ID not found in config"))?;

        let auth_token = get_auth_token(config)
            .ok_or_else(|| anyhow::anyhow!("Auth token not found in config"))?;

        let mqtt_config = config
            .remote_mqtt_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Remote MQTT config not found"))?;

        // Create MQTT client options
        let create_opts = mqtt::CreateOptionsBuilder::new()
            .server_uri(&mqtt_config.mqtt_instance_url)
            .client_id(&mqtt_config.mqtt_client_id)
            .finalize();

        let client = mqtt::AsyncClient::new(create_opts).context("Failed to create MQTT client")?;

        // Get the receiver before connecting
        let rx = client.start_consuming();

        // Build connection options
        let mut conn_opts_builder = mqtt::ConnectOptionsBuilder::new();
        conn_opts_builder
            .keep_alive_interval(Duration::from_secs(60))
            .clean_session(true)
            .automatic_reconnect(Duration::from_secs(1), Duration::from_secs(60));

        // Set credentials if provided
        if let Some(username) = &mqtt_config.mqtt_username {
            let password = mqtt_config.mqtt_password.as_deref().unwrap_or("");
            conn_opts_builder.user_name(username).password(password);
        } else {
            // Use device auth token as username
            conn_opts_builder
                .user_name(&device_id)
                .password(&auth_token);
        }

        let conn_opts = conn_opts_builder.finalize();

        // Connect to the broker
        client
            .connect(conn_opts)
            .wait()
            .context("Failed to connect to MQTT broker")?;

        info!(
            "Connected to MQTT broker at {} with namespace: {}",
            mqtt_config.mqtt_instance_url, mqtt_config.mqtt_namespace
        );

        Ok((
            Self {
                client,
                device_id,
                namespace: mqtt_config.mqtt_namespace.clone(),
                start_time: std::time::Instant::now(),
                inbound,
            },
            rx,
        ))
    }

    /// Subscribe to device command topics
    pub fn subscribe_to_commands(&self) -> Result<()> {
        let name_topic = format!("{}/devices/{}/name", self.namespace, self.device_id);
        let toolguard_topic = format!(
            "{}/devices/{}/toolguard/state",
            self.namespace, self.device_id
        );
        let doors_state_topic =
            format!("{}/devices/{}/doors/state", self.namespace, self.device_id);
        let doors_unlock_topic =
            format!("{}/devices/{}/doors/unlock", self.namespace, self.device_id);
        let power_state_topic =
            format!("{}/devices/{}/power/state", self.namespace, self.device_id);

        self.client
            .subscribe(&name_topic, 1)
            .wait()
            .context("Failed to subscribe to name update topic")?;

        self.client
            .subscribe(&toolguard_topic, 1)
            .wait()
            .context("Failed to subscribe to toolguard state topic")?;

        // Retained snapshots are nice but not required; standard QoS 1.
        self.client
            .subscribe(&doors_state_topic, 1)
            .wait()
            .context("Failed to subscribe to doors/state topic")?;
        self.client
            .subscribe(&doors_unlock_topic, 1)
            .wait()
            .context("Failed to subscribe to doors/unlock topic")?;
        self.client
            .subscribe(&power_state_topic, 1)
            .wait()
            .context("Failed to subscribe to power/state topic")?;

        info!(
            "Subscribed to command topics with namespace: {}",
            self.namespace
        );
        Ok(())
    }

    /// Publish a door access event (`doors/event`) back to the server.
    /// Called by the bridge task when the local broker reports a scan.
    pub fn publish_doors_event(&self, event: &DoorsEvent) -> Result<()> {
        let topic = format!("{}/devices/{}/doors/event", self.namespace, self.device_id);
        let payload = serde_json::to_vec(event).context("Failed to serialize DoorsEvent")?;
        let msg = mqtt::Message::new(topic, payload, 1);
        self.client
            .publish(msg)
            .wait()
            .context("Failed to publish doors/event")?;
        Ok(())
    }

    /// Publish heartbeat message
    pub fn publish_heartbeat(&self) -> Result<()> {
        let topic = format!("{}/devices/{}/heartbeat", self.namespace, self.device_id);
        let msg = mqtt::Message::new(topic, "ping", 1);

        self.client
            .publish(msg)
            .wait()
            .context("Failed to publish heartbeat")?;

        debug!("Published heartbeat");
        Ok(())
    }

    /// Publish device data
    pub fn publish_device_data(&self) -> Result<()> {
        let topic = format!("{}/devices/{}/data", self.namespace, self.device_id);

        // Update system info (IP addresses may change)
        let current_info = get_system_info()?;

        let data = DeviceData {
            mac_address: current_info.mac_address,
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            ipv4_address: current_info.ipv4_address,
            ipv6_address: current_info.ipv6_address,
            uptime: self.start_time.elapsed().as_secs(),
            platform: current_info.platform,
        };

        let payload = serde_json::to_string(&data).context("Failed to serialize device data")?;

        let msg = mqtt::Message::new(topic, payload, 1);
        self.client
            .publish(msg)
            .wait()
            .context("Failed to publish device data")?;

        info!("Published device data");
        Ok(())
    }

    /// Start heartbeat task (publishes every 15s)
    pub fn start_heartbeat_task(&self) {
        let mut heartbeat_interval = interval(Duration::from_secs(15)); // 3 minutes
        let client = self.client.clone();
        let device_id = self.device_id.clone();
        let namespace = self.namespace.clone();

        tokio::spawn(async move {
            loop {
                heartbeat_interval.tick().await;

                let topic = format!("{}/devices/{}/heartbeat", namespace, device_id);
                let msg = mqtt::Message::new(topic, "ping", 1);

                if let Err(e) = client.publish(msg).wait() {
                    error!("Failed to publish heartbeat: {}", e);
                } else {
                    debug!("Heartbeat published");
                }
            }
        });

        info!("Heartbeat task started (interval: 3 minutes)");
    }

    /// Start device data publishing task (publishes every 7.5 minutes)
    pub fn start_data_publisher_task(&self) {
        let mut data_interval = interval(Duration::from_secs(450)); // 7.5 minutes
        let client = self.client.clone();
        let device_id = self.device_id.clone();
        let namespace = self.namespace.clone();
        let start_time = self.start_time;

        tokio::spawn(async move {
            loop {
                data_interval.tick().await;

                // Collect current system info
                match get_system_info() {
                    Ok(info) => {
                        let data = DeviceData {
                            mac_address: info.mac_address,
                            software_version: env!("CARGO_PKG_VERSION").to_string(),
                            ipv4_address: info.ipv4_address,
                            ipv6_address: info.ipv6_address,
                            uptime: start_time.elapsed().as_secs(),
                            platform: info.platform,
                        };

                        match serde_json::to_string(&data) {
                            Ok(payload) => {
                                let topic = format!("{}/devices/{}/data", namespace, device_id);
                                let msg = mqtt::Message::new(topic, payload, 1);

                                if let Err(e) = client.publish(msg).wait() {
                                    error!("Failed to publish device data: {}", e);
                                } else {
                                    info!("Device data published");
                                }
                            }
                            Err(e) => {
                                error!("Failed to serialize device data: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to collect system info: {}", e);
                    }
                }
            }
        });

        info!("Data publisher task started (interval: 15 minutes)");
    }

    /// Handle incoming MQTT messages. The topic suffix is the same
    /// `WireMessage::kind` string the WebSocket transport uses, so we just
    /// extract it and delegate to the shared inbound dispatcher.
    pub async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<()> {
        let prefix = format!("{}/devices/{}/", self.namespace, self.device_id);
        let suffix = match topic.strip_prefix(&prefix) {
            Some(s) => s,
            None => {
                warn!("Received message on unexpected topic: {}", topic);
                return Ok(());
            }
        };
        self.inbound.dispatch(suffix, payload).await;
        Ok(())
    }

    /// Disconnect from MQTT broker gracefully
    pub fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from MQTT broker...");
        self.client
            .disconnect(None)
            .wait()
            .context("Failed to disconnect from MQTT broker")?;
        info!("Disconnected from MQTT broker");
        Ok(())
    }
}

/// Run the MQTT message receiver loop
pub async fn run_mqtt_event_loop(
    rx: mqtt::Receiver<Option<mqtt::Message>>,
    mqtt_client: std::sync::Arc<EdgeMqttClient>,
) -> Result<()> {
    info!("Starting MQTT event loop");

    // Spawn the receiver task in a blocking thread so it doesn't block tokio runtime
    let mqtt_client_clone = mqtt_client.clone();
    let receiver_task = tokio::task::spawn_blocking(move || {
        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(Some(msg)) => {
                    debug!("Received message on topic: {}", msg.topic());

                    // Handle the message by spawning a new async task
                    let mqtt_client = mqtt_client_clone.clone();
                    let topic = msg.topic().to_string();
                    let payload = msg.payload().to_vec();

                    tokio::spawn(async move {
                        if let Err(e) = mqtt_client.handle_message(&topic, &payload).await {
                            error!("Error handling message: {}", e);
                        }
                    });
                }
                Ok(None) => {
                    warn!("MQTT connection lost, waiting for reconnection...");
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
                Err(e) => {
                    // Check if it's a timeout (normal) or disconnect (error)
                    if e.is_timeout() {
                        // Timeout is normal, continue loop
                        continue;
                    } else {
                        error!("MQTT receiver disconnected");
                        break;
                    }
                }
            }
        }
    });

    // Wait for the receiver task to complete
    match receiver_task.await {
        Ok(_) => {
            info!("MQTT receiver task completed");
        }
        Err(e) => {
            error!("MQTT receiver task panicked: {}", e);
        }
    }

    Ok(())
}

// ── Local MQTT client ─────────────────────────────────────────────────────────
//
// The topic vocabulary lives in `css_lib::wire::local` rather than here: this
// edge is one of four crates that speak it, and a private copy per crate is how
// `toolguard-test-ui` came to be a release behind without anything noticing.

/// Tool-on and tool-off, published by local hardware onto the local broker.
/// They carry nothing but the card and the tool.
///
/// `seconds` and `temperature` used to live here too, because one struct parsed
/// all three tool topics. Their union is what a per-topic documentation check
/// sees, so nothing could tell whether tool-on had grown a duration or tool-log
/// had lost one. Tool-log now has its own named type in `css_lib::wire` (#169)
/// and this one is exactly the two topics it serves.
#[derive(Debug, Deserialize)]
struct LocalToolRequest {
    card: String,
    tool_id: String,
}

/// Tool-off on its own (#179): the card is optional, because a stop is a
/// safety act and needs no badge. A plug with a stop button, a sensor, or an
/// automation on the local broker names the tool alone, and the server ends
/// whatever session is open on it. Tool-on keeps its own type above: it always
/// names who is asking.
#[derive(Debug, Deserialize)]
struct LocalToolStopRequest {
    #[serde(default)]
    card: Option<String>,
    tool_id: String,
}

/// A firmware power reading on the local broker (#48). The edge records it for
/// local aggregation, relays it to the server, and (on a local overage) trips.
#[derive(Debug, Deserialize)]
struct LocalPowerRequest {
    tool_id: String,
    /// Who is reporting. Optional so older firmware still parses, but a module
    /// that omits it cannot be credited with being alive -- see
    /// `ModuleState::note_device_seen`.
    #[serde(default)]
    device_id: Option<String>,
    /// The module's own view of its output. `None` means "cannot report", which
    /// is NOT the same as `false`: the bypass detector treats unknown as
    /// unknown, and a plug that claimed `false` when it did not know would
    /// silently disarm the check that exists to catch a bypassed tool.
    #[serde(default)]
    relay_on: Option<bool>,
    #[serde(default)]
    draw_now: Option<f64>,
    #[serde(default)]
    voltage_now: Option<f64>,
    #[serde(default)]
    max_voltage: Option<f64>,
    #[serde(default)]
    amperage_limit: Option<f64>,
    #[serde(default)]
    self_tripped: Option<bool>,
}

pub struct LocalMqttClient {
    client: mqtt::AsyncClient,
    toolguard_state: Arc<ToolGuardState>,
    power_state: Arc<PowerState>,
    remote_instance_url: String,
    remote_auth_token: String,
    http_client: Client,
    refresh_notify: Arc<tokio::sync::Notify>,
    /// Forwards local scan outcomes back to the server as `doors/event`. The
    /// receiver is consumed by a task in `main`.
    doors_event_tx: DoorsEventSender,
    /// Wiring + interlocks. Written here only to record module liveness; the
    /// lease watchdog in `main` is what reads it.
    module_state: Arc<crate::modules::ModuleState>,
    /// #120 (#121): per-device command-channel HMAC key, used to sign outbound
    /// `doors/event`. `None` on a legacy device -> events go unsigned.
    command_key: Option<Vec<u8>>,
    /// The one cache (#182): what a badge at anything is decided against, and
    /// where every session this edge opens lives.
    cache: Arc<ResourceCache>,
}

impl LocalMqttClient {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        mqtt_config: &MqttConfig,
        toolguard_state: Arc<ToolGuardState>,
        power_state: Arc<PowerState>,
        remote_instance_url: String,
        remote_auth_token: String,
        doors_event_tx: DoorsEventSender,
        module_state: Arc<crate::modules::ModuleState>,
        command_key: Option<Vec<u8>>,
        cache: Arc<ResourceCache>,
    ) -> Result<(Self, mqtt::Receiver<Option<mqtt::Message>>)> {
        let create_opts = mqtt::CreateOptionsBuilder::new()
            .server_uri(&mqtt_config.mqtt_instance_url)
            .client_id(&mqtt_config.mqtt_client_id)
            .finalize();

        let client =
            mqtt::AsyncClient::new(create_opts).context("Failed to create local MQTT client")?;

        let rx = client.start_consuming();

        let mut conn_opts_builder = mqtt::ConnectOptionsBuilder::new();
        conn_opts_builder
            .keep_alive_interval(Duration::from_secs(60))
            .clean_session(true)
            .automatic_reconnect(Duration::from_secs(1), Duration::from_secs(60));

        if let Some(username) = &mqtt_config.mqtt_username {
            let password = mqtt_config.mqtt_password.as_deref().unwrap_or("");
            conn_opts_builder.user_name(username).password(password);
        }

        let conn_opts = conn_opts_builder.finalize();
        client
            .connect(conn_opts)
            .wait()
            .context("Failed to connect to local MQTT broker")?;

        info!(
            "Connected to local MQTT broker at {}",
            mqtt_config.mqtt_instance_url
        );

        Ok((
            Self {
                client,
                toolguard_state,
                power_state,
                remote_instance_url,
                remote_auth_token,
                http_client: Client::new(),
                refresh_notify: Arc::new(tokio::sync::Notify::new()),
                doors_event_tx,
                module_state,
                command_key,
                cache,
            },
            rx,
        ))
    }

    pub fn subscribe_to_requests(&self) -> Result<()> {
        self.client
            .subscribe(local::TOOL_ON_REQUEST, 1)
            .wait()
            .context("Failed to subscribe to tool-on requests")?;
        self.client
            .subscribe(local::TOOL_OFF_REQUEST, 1)
            .wait()
            .context("Failed to subscribe to tool-off requests")?;
        self.client
            .subscribe(local::TOOL_LOG_REQUEST, 1)
            .wait()
            .context("Failed to subscribe to tool-log requests")?;
        self.client
            .subscribe(local::POWER_REQUEST, 1)
            .wait()
            .context("Failed to subscribe to power requests")?;
        self.client
            .subscribe(local::ALIVE_REQUEST, 1)
            .wait()
            .context("Failed to subscribe to alive pulses")?;
        self.client
            .subscribe(local::KIOSK_REFRESH_REQUEST, 0)
            .wait()
            .context("Failed to subscribe to kiosk refresh topic")?;
        self.client
            .subscribe(local::DOOR_SCAN_REQUEST, 1)
            .wait()
            .context("Failed to subscribe to door scan requests")?;
        // The resource-shaped wire (#182). The topics above are its aliases
        // for one release; these are the wire.
        for (topic, qos) in [
            (local::SCAN_REQUEST, 1),
            (local::STOP_REQUEST, 1),
            (local::USAGE_REQUEST, 1),
            (local::POWER_REPORT_REQUEST, 1),
            (local::RESOURCE_ALIVE_REQUEST, 1),
        ] {
            self.client
                .subscribe(topic, qos)
                .wait()
                .with_context(|| format!("Failed to subscribe to {topic}"))?;
        }
        info!("Subscribed to local resource request topics (and their toolguard/door aliases)");
        Ok(())
    }

    pub async fn handle_message(&self, topic: &str, payload: &[u8]) {
        if topic == local::KIOSK_REFRESH_REQUEST {
            self.handle_refresh_request().await;
            return;
        }

        if topic == local::DOOR_SCAN_REQUEST {
            match serde_json::from_slice::<LocalScanRequest>(payload) {
                Ok(req) => self.handle_door_scan(req).await,
                Err(e) => warn!("Failed to parse door scan request: {}", e),
            }
            return;
        }

        // ── the resource-shaped wire (#182) ──────────────────────────────
        //
        // One scan, one stop, one usage report, one power report, naming the
        // resource. Each is translated into the handler the alias topic has
        // always used, so there is exactly one decision path per message
        // whichever name it arrived under.
        if topic == local::SCAN_REQUEST {
            match serde_json::from_slice::<LocalScan>(payload) {
                Ok(req) => self.handle_scan(req).await,
                Err(e) => warn!("Failed to parse {} : {}", topic, e),
            }
            return;
        }
        if topic == local::STOP_REQUEST {
            match serde_json::from_slice::<LocalStop>(payload) {
                Ok(req) => {
                    self.handle_tool_off(LocalToolStopRequest {
                        card: req.card,
                        tool_id: req.resource,
                    })
                    .await
                }
                Err(e) => warn!("Failed to parse {} : {}", topic, e),
            }
            return;
        }
        if topic == local::USAGE_REQUEST {
            match serde_json::from_slice::<LocalUsage>(payload) {
                Ok(req) => {
                    self.handle_tool_log(LocalToolLogRequest {
                        card: req.card,
                        tool_id: req.resource,
                        seconds: req.seconds,
                        active_seconds: req.active_seconds,
                        temperature: req.temperature,
                    })
                    .await
                }
                Err(e) => warn!("Failed to parse {} : {}", topic, e),
            }
            return;
        }
        if topic == local::POWER_REPORT_REQUEST {
            match serde_json::from_slice::<LocalPower>(payload) {
                Ok(req) => {
                    self.handle_power(LocalPowerRequest {
                        tool_id: req.resource,
                        device_id: req.device_id,
                        relay_on: req.relay_on,
                        draw_now: req.draw_now,
                        voltage_now: req.voltage_now,
                        max_voltage: req.max_voltage,
                        amperage_limit: req.amperage_limit,
                        self_tripped: req.self_tripped,
                    })
                    .await
                }
                Err(e) => warn!("Failed to parse {} : {}", topic, e),
            }
            return;
        }
        if topic == local::RESOURCE_ALIVE_REQUEST {
            match serde_json::from_slice::<LocalAliveRequest>(payload) {
                Ok(req) => self.handle_alive(req).await,
                Err(e) => warn!("Failed to parse {} : {}", topic, e),
            }
            return;
        }

        // ── the aliases: still served, one release, and said so once ─────
        self.note_deprecated_topic(topic);

        if topic == local::POWER_REQUEST {
            match serde_json::from_slice::<LocalPowerRequest>(payload) {
                Ok(req) => self.handle_power(req).await,
                Err(e) => warn!("Failed to parse local power request: {}", e),
            }
            return;
        }

        // The pulse (#170). Any device, any role: this is what credits a device
        // with being alive, and a power report is the only other thing that does.
        if topic == local::ALIVE_REQUEST {
            match serde_json::from_slice::<LocalAliveRequest>(payload) {
                Ok(req) => self.handle_alive(req).await,
                Err(e) => warn!("Failed to parse local alive pulse: {}", e),
            }
            return;
        }

        // Tool-log is parsed as its own shape, because it is the only one of
        // the three that carries durations (#169).
        if topic == local::TOOL_LOG_REQUEST {
            match serde_json::from_slice(payload) {
                Ok(req) => self.handle_tool_log(req).await,
                Err(e) => warn!("Failed to parse local tool request on {}: {}", topic, e),
            }
            return;
        }

        if topic == local::TOOL_OFF_REQUEST {
            match serde_json::from_slice::<LocalToolStopRequest>(payload) {
                Ok(req) => self.handle_tool_off(req).await,
                Err(e) => warn!("Failed to parse local tool request on {}: {}", topic, e),
            }
            return;
        }

        let req: LocalToolRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to parse local tool request on {}: {}", topic, e);
                return;
            }
        };

        match topic {
            local::TOOL_ON_REQUEST => self.handle_tool_on(req).await,
            _ => {}
        }
    }

    /// The deprecated door alias (#182): `door/request/scan` is a scan.
    async fn handle_door_scan(&self, req: LocalScanRequest) {
        self.handle_scan(LocalScan {
            resource: req.door_id.to_string(),
            card: req.card_id,
        })
        .await;
    }

    /// Tell the server what happened at a door (audit log, webhook, etc.): the
    /// signed `doors/event` the server's door path records.
    fn report_door_event(
        &self,
        door_id: uuid::Uuid,
        card: String,
        granted: bool,
        reason: Option<String>,
    ) {
        let mut event = doors::DoorsEvent {
            door_id,
            card_id: Some(card),
            granted,
            reason,
            source: "rfid",
            occurred_at: chrono::Utc::now(),
            sig: None,
        };
        // #120 (#121): sign the event so the server rejects a forged doors/event.
        if let Some(key) = &self.command_key {
            let msg = css_lib::sig::doors_event_message(
                &event.door_id.to_string(),
                event.card_id.as_deref().unwrap_or(""),
                event.granted,
                event.source,
            );
            event.sig = Some(css_lib::sig::sign(key, &msg));
        }
        if self.doors_event_tx.send(event).is_err() {
            warn!("Doors event bridge channel closed; cannot report scan upstream");
        }
    }

    /// Forward a server-initiated unlock command onto the local broker so
    /// the relay actuates. Called by a bridge task in `main` that drains the
    /// channel populated by [`EdgeMqttClient`].
    pub fn publish_doors_unlock_response(&self, cmd: &UnlockCommand) {
        self.publish_unlock(&LocalUnlockResponse {
            door_id: cmd.door_id,
            granted: true,
            duration_ms: cmd.duration_ms,
            reason: Some(cmd.reason.clone()),
        });
    }

    /// Every door unlock -- a scan's answer, a server-initiated unlock, an Open
    /// Access hold pulse -- goes out on both wires (#182): the alias
    /// `door/response/unlock` as it always has, and `resource/response/scan`
    /// with the pulse carried as `strike`. One place, so the two cannot drift.
    fn publish_unlock(&self, response: &LocalUnlockResponse) {
        if let Ok(v) = serde_json::to_value(response) {
            self.publish_local(local::DOOR_UNLOCK_RESPONSE, &v);
        }
        let reply = LocalScanReply::from_door(
            &response.door_id.to_string(),
            response.granted,
            response.duration_ms,
            response.reason.as_deref(),
        );
        if let Ok(v) = serde_json::to_value(&reply) {
            self.publish_local(local::SCAN_RESPONSE, &v);
        }
    }

    /// A card presented to a resource (#182): the one path, whatever the
    /// resource is and whichever topic the badge arrived on.
    ///
    /// One decision ([`ResourceCache::decide`]), one session opened on a yes
    /// with the resource's profile, one reply -- published on the resource
    /// topic and, for one release, on the alias the resource's kind used to
    /// answer on -- and one report upstream: a door's goes as the signed
    /// `doors/event` the server's door path records; a tool's opens the
    /// session on the server's resource path. A tool the server insists on
    /// deciding itself is asked over that same path, and refused if it cannot
    /// be asked.
    async fn handle_scan(&self, req: LocalScan) {
        let now = chrono::Utc::now();
        let kind = self
            .cache
            .resolve(&req.resource)
            .map(|r| r.kind)
            .unwrap_or(Kind::Tool);
        let (authorized, reason, strike_ms) = match self.cache.decide(&req.resource, &req.card, now)
        {
            ScanDecision::Allow { strike_ms } => (true, "authorized".to_string(), strike_ms),
            ScanDecision::Deny(why) => (false, why, None),
            ScanDecision::AskServer => self.remote_scan(&req).await,
        };

        self.publish_scan_reply(&req, kind, authorized, &reason, strike_ms);

        if authorized {
            // The session this edge just opened is what a lease is for (#179),
            // and the resource's profile says how long it lasts (#182).
            self.cache.open_session(&req.resource, now);
        }

        match kind {
            Kind::Door => {
                if let Ok(door_id) = uuid::Uuid::parse_str(&req.resource) {
                    let why = (!authorized).then(|| reason.clone());
                    self.report_door_event(door_id, req.card, authorized, why);
                }
            }
            Kind::Tool => {
                // Decided here: best-effort forward so the server opens the
                // session (and, for a metered tool in edge-local mode, places
                // the hold and re-broadcasts). Decided by the server: it
                // already has.
                if authorized && !self.toolguard_state.tool_requires_online(&req.resource) {
                    self.forward_scan(req.resource.clone(), req.card.clone());
                }
            }
        }
    }

    /// The one reply to a scan, on the resource topic and on the alias the
    /// resource's kind used to answer on (#182, one release).
    fn publish_scan_reply(
        &self,
        req: &LocalScan,
        kind: Kind,
        authorized: bool,
        reason: &str,
        strike_ms: Option<i32>,
    ) {
        match (kind, uuid::Uuid::parse_str(&req.resource)) {
            (Kind::Door, Ok(door_id)) => self.publish_unlock(&LocalUnlockResponse {
                door_id,
                granted: authorized,
                duration_ms: strike_ms.unwrap_or(0),
                reason: (!authorized).then(|| reason.to_string()),
            }),
            // The request's own name, echoed: a reply names the resource that
            // was asked about, or a module cannot tell it from its neighbour's.
            _ => self.publish_tool_on(&req.resource, authorized, reason, strike_ms),
        }
    }

    /// Open a tool's session on the server after a local yes: the resource
    /// path (#182), fire-and-forget as the alias forward always was.
    fn forward_scan(&self, resource: String, card: String) {
        let url = format!(
            "{}/api/resources/{}/scan",
            self.remote_instance_url, resource
        );
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        tokio::spawn(async move {
            // A JSON body, not a query string: the card identifies a person
            // and a URL is written down by every proxy and access log it
            // passes through (#107).
            let body = serde_json::json!({ "card": card });
            if let Err(e) = http.post(&url).bearer_auth(&token).json(&body).send().await {
                warn!("Failed to forward scan to remote: {}", e);
            }
        });
    }

    /// Say once per alias topic that it is on its way out (#182). Once, not
    /// per message: a plug heartbeats every second.
    fn note_deprecated_topic(&self, topic: &str) {
        use std::collections::HashSet;
        use std::sync::{Mutex, OnceLock};
        static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let warned = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
        let mut set = warned.lock().expect("deprecation set");
        if set.insert(topic.to_string()) {
            warn!(
                "{topic} is a deprecated alias; the resource-shaped topic replaces it in the \
                 next minor release (see FIRMWARE.md, the local broker)"
            );
        }
    }

    /// Publish one Open Access hold pulse: a momentary unlock the hold-refresh
    /// loop re-sends before it elapses to keep the strike energized for the whole
    /// window. Same wire message as any unlock (the external relay firmware only
    /// understands the momentary `duration_ms`); holding is a property of the
    /// loop re-sending, and stopping is what relocks the door — fail-secure.
    pub fn publish_doors_hold(&self, door_id: uuid::Uuid, duration_ms: i32) {
        self.publish_unlock(&LocalUnlockResponse {
            door_id,
            granted: true,
            duration_ms,
            reason: Some("open_access".to_string()),
        });
    }

    /// Publish one lease decision onto the local broker.
    ///
    /// Deliberately **not retained**. A retained lease is re-delivered to any
    /// module that connects afterwards, which would hand a plug rejoining the
    /// broker minutes later a grant the coordinator issued before the fault --
    /// the stale promise the whole mechanism exists to make impossible.
    ///
    /// Failures are not retried here. A lease that does not arrive is the same
    /// event as a lease that was never sent: the module's own watchdog expires
    /// and it de-energizes. Retrying a missed renewal would only narrow the
    /// window in which that correct behaviour happens.
    /// Hand the server's view of which tools are in use to the lease engine
    /// (#179): a session opened elsewhere -- or ended by an administrator or
    /// the sweep -- reaches this edge through the sync.
    pub fn reconcile_sessions(&self, payload: &crate::toolguard::SyncPayload) {
        self.cache.reconcile(payload, chrono::Utc::now());
    }

    pub fn publish_tool_lease(&self, action: &crate::modules::LeaseAction) {
        match serde_json::to_value(action) {
            Ok(v) => self.publish_local(local::TOOL_LEASE, &v),
            Err(e) => warn!("Failed to serialize tool lease: {}", e),
        }
        // The same lease, resource-shaped (#182).
        match serde_json::to_value(ResourceLeasePayload::from(action)) {
            Ok(v) => self.publish_local(local::RESOURCE_LEASE, &v),
            Err(e) => warn!("Failed to serialize resource lease: {}", e),
        }
    }

    pub async fn handle_refresh_request(&self) {
        debug!("Kiosk refresh request received");
        if let Some(payload) = self.toolguard_state.get_state() {
            match serde_json::to_vec(&payload) {
                Ok(bytes) => self.publish_state_bytes(bytes),
                Err(e) => warn!("Failed to serialize toolguard state for refresh: {}", e),
            }
        }
        self.refresh_notify.notify_waiters();
    }

    /// The deprecated tool alias (#182): `toolguard/request/tool-on` is a scan.
    async fn handle_tool_on(&self, req: LocalToolRequest) {
        self.handle_scan(LocalScan {
            resource: req.tool_id,
            card: req.card,
        })
        .await;
    }

    /// A firmware power reading (#48): record it for edge-local aggregation, trip
    /// + report any circuit now over its amperage limit, forward the reading to
    /// the server (which aggregates too), and ack. Decimal fields go up as
    /// strings, matching the server's tested `power-report` wire format.
    async fn handle_power(&self, req: LocalPowerRequest) {
        // A power report is this module saying it is alive, and it is the only
        // thing that says so on the local broker. Without it every binding whose
        // `on_disconnect` is `fail_off` -- the default, and the right default --
        // looks permanently silent, so the interlock engine refuses it a lease
        // forever and the tool never energizes.
        if let Some(device_id) = req.device_id.as_deref() {
            let marked = self
                .module_state
                .note_device_seen(device_id, chrono::Utc::now());
            if marked == 0 {
                warn!(
                    "Power report from device {} matched no module binding; it \
                     will be refused a lease until its wiring arrives",
                    device_id
                );
            }
        } else {
            debug!(
                "Power report for {} carries no device_id, so no module can be \
                 credited as alive by it",
                req.tool_id
            );
        }

        // The relay state is what confirms, for start sequencing, that this
        // device actually energized (#179). Absent is "cannot tell", and a
        // device that cannot tell can never confirm.
        if let (Some(device_id), Some(relay_on)) = (req.device_id.as_deref(), req.relay_on) {
            self.module_state
                .note_relay_state(device_id, relay_on, chrono::Utc::now());
        }

        if let Some(draw) = req.draw_now {
            self.power_state
                .record_draw(&req.tool_id, draw, chrono::Utc::now());
        }

        // Edge-local fast-trip: any circuit now over its limit is tripped locally
        // (its tools denied immediately via the sticky cache) and reported up.
        for circuit_id in self.power_state.evaluate_overages() {
            self.power_state.mark_tripped(&circuit_id);
            warn!(
                "Edge fast-trip: circuit {} over its amperage limit",
                circuit_id
            );
            let url = format!("{}/api/toolguard/power-trip", self.remote_instance_url);
            let token = self.remote_auth_token.clone();
            let http = self.http_client.clone();
            let body = serde_json::json!({
                "circuit_id": circuit_id,
                "reason": "edge-local aggregation over amperage limit",
            });
            tokio::spawn(async move {
                if let Err(e) = http.post(&url).bearer_auth(&token).json(&body).send().await {
                    warn!("Failed to report edge fast-trip to remote: {}", e);
                }
            });
        }

        // Relay the reading to the server (it aggregates + records latest draw).
        let url = format!("{}/api/toolguard/power-report", self.remote_instance_url);
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        let body = serde_json::json!({
            "tool_id": req.tool_id,
            "draw_now": req.draw_now.map(|v| v.to_string()),
            "voltage_now": req.voltage_now.map(|v| v.to_string()),
            "max_voltage": req.max_voltage.map(|v| v.to_string()),
            "amperage_limit": req.amperage_limit.map(|v| v.to_string()),
            "self_tripped": req.self_tripped,
            // #161: name the MODULE, not just this edge. Without it the server
            // can only refresh its own last_seen, and the module -- which has no
            // site-broker credentials and never will -- has no liveness at all
            // from the server's side. Absent stays absent: an older server
            // ignores the field, and a module that sent no device_id contributes
            // none, which is what `note_device_seen` above already told us.
            "device_id": req.device_id,
            // Forwarded rather than dropped: #84's relay oracle reads this, and
            // until now a module reporting over the local broker could never
            // make it fire at all. Absent stays absent -- serde_json renders
            // None as null and the server's `Option<bool>` keeps the three-way
            // distinction the detector depends on.
            "relay_on": req.relay_on,
        });
        tokio::spawn(async move {
            if let Err(e) = http.post(&url).bearer_auth(&token).json(&body).send().await {
                warn!("Failed to forward power report to remote: {}", e);
            }
        });

        self.publish_local(local::POWER_RESPONSE, &serde_json::json!({ "ok": true }));
        self.publish_local(
            local::POWER_REPORT_RESPONSE,
            &serde_json::json!({ "ok": true }),
        );
    }

    /// Synchronously ask the server to decide a scan (and, for a metered tool,
    /// place the hold) on its resource path (#182). Returns `(authorized,
    /// reason, strike_ms)`. Any transport or non-2xx error is a denial -- fail
    /// closed.
    async fn remote_scan(&self, req: &LocalScan) -> (bool, String, Option<i32>) {
        let url = format!(
            "{}/api/resources/{}/scan",
            self.remote_instance_url, req.resource
        );
        let body = serde_json::json!({ "card": req.card });
        match self
            .http_client
            .post(&url)
            .bearer_auth(&self.remote_auth_token)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => match resp.json::<serde_json::Value>().await {
                Ok(body) => {
                    // The server's envelope for a scan is { status, message,
                    // authorized, strike? }; `tool_on` is the same bit under
                    // the alias name, read in case an older server answers.
                    let authorized = body
                        .get("authorized")
                        .or_else(|| body.get("tool_on"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let reason = body
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(if authorized { "authorized" } else { "denied" })
                        .to_string();
                    let strike_ms = body
                        .get("strike")
                        .and_then(|s| s.get("pulse_ms"))
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32);
                    (authorized, reason, strike_ms)
                }
                Err(e) => (false, format!("server response unreadable: {e}"), None),
            },
            Err(e) => {
                warn!("online-sync scan to remote failed: {}", e);
                (false, "server unreachable".to_string(), None)
            }
        }
    }

    async fn handle_tool_off(&self, req: LocalToolStopRequest) {
        let response_payload = serde_json::json!({ "ok": true });
        self.publish_local(local::TOOL_OFF_RESPONSE, &response_payload);
        self.publish_local(local::STOP_RESPONSE, &response_payload);
        // The lease ends with the stop, not a sync interval later (#179).
        self.cache.close_session(&req.tool_id);

        let url = format!(
            "{}/api/resources/{}/stop",
            self.remote_instance_url, req.tool_id
        );
        let card = req.card.clone();
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        tokio::spawn(async move {
            let mut body = serde_json::json!({});
            // Forwarded only when the device sent one (#179); the server ends
            // the open session either way.
            if let Some(c) = card {
                body["card"] = serde_json::json!(c);
            }
            if let Err(e) = http.post(&url).bearer_auth(&token).json(&body).send().await {
                warn!("Failed to forward tool-off to remote: {}", e);
            }
        });
    }

    /// A device said it is alive (#170).
    ///
    /// Two effects, for two audiences. The edge credits every binding the device
    /// holds, which is what the lease engine's `fail_off` check reads -- so a
    /// reader or sensor that could never send a power report can now be heard
    /// from at all. And the pulse is forwarded so the SERVER's `last_seen_at` is
    /// written for the device too: that column is what #84's silence detector
    /// and the admin device list read, and a forwarded power report could never
    /// write it for a non-power device, because the server gates that relay on
    /// the `power` binding.
    ///
    /// Forwarded on every pulse, matching what the power report does today. At
    /// a pulse a second per device that is far more than the server's 90 s
    /// silence threshold needs; a per-device throttle on the forward is the
    /// obvious follow-up if the request rate ever matters, and it belongs here
    /// rather than in firmware, which should not have to know the server's
    /// thresholds.
    async fn handle_alive(&self, req: LocalAliveRequest) {
        let marked = self
            .module_state
            .note_device_seen(&req.device_id, chrono::Utc::now());
        if marked == 0 {
            warn!(
                "Alive pulse from device {} matched no module binding; it will be \
                 refused a lease until its wiring arrives",
                req.device_id
            );
        }

        let url = format!("{}/api/toolguard/alive", self.remote_instance_url);
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        let device_id = req.device_id;
        tokio::spawn(async move {
            let body = serde_json::json!({ "device_id": device_id });
            if let Err(e) = http.post(&url).bearer_auth(&token).json(&body).send().await {
                warn!("Failed to forward alive pulse to remote: {}", e);
            }
        });
    }

    async fn handle_tool_log(&self, req: LocalToolLogRequest) {
        let seconds = req.seconds.unwrap_or(0.0);
        let response_payload = serde_json::json!({ "ok": true });
        self.publish_local(local::TOOL_LOG_RESPONSE, &response_payload);
        self.publish_local(local::USAGE_RESPONSE, &response_payload);

        let url = format!(
            "{}/api/resources/{}/usage",
            self.remote_instance_url, req.tool_id
        );
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        let card = req.card.clone();
        let temperature = req.temperature;
        let active_seconds = req.active_seconds;
        tokio::spawn(async move {
            let mut body = serde_json::json!({ "seconds": seconds });
            // Forwarded only when the module sent one. A plug names no card
            // (#174); the server attributes its report through the tool's
            // session, and an invented card here would be refused as unknown.
            if let Some(c) = card {
                body["card"] = serde_json::json!(c);
            }
            // Forwarded only when the module sent it: absent means "this device
            // senses no working signal", which is not the same as zero.
            if let Some(a) = active_seconds {
                body["active_seconds"] = serde_json::json!(a);
            }
            if let Some(t) = temperature {
                body["temperature"] = serde_json::json!(t);
            }
            if let Err(e) = http.post(&url).bearer_auth(&token).json(&body).send().await {
                warn!("Failed to forward tool-log to remote: {}", e);
            }
        });
    }

    /// Publish the current toolguard state to the local broker so subscribers
    /// (e.g. status kiosks) receive it immediately.
    pub fn publish_state_bytes(&self, bytes: Vec<u8>) {
        // Both names, one release (#182).
        for topic in [local::STATE, local::RESOURCE_STATE] {
            let msg = mqtt::Message::new(topic, bytes.clone(), 1);
            if let Err(e) = self.client.publish(msg).wait() {
                warn!("Failed to publish state to {}: {}", topic, e);
            }
        }
    }

    fn publish_local(&self, topic: &str, payload: &serde_json::Value) {
        match serde_json::to_string(payload) {
            Ok(json) => {
                let msg = mqtt::Message::new(topic, json, 1);
                if let Err(e) = self.client.publish(msg).wait() {
                    warn!("Failed to publish local response to {}: {}", topic, e);
                }
            }
            Err(e) => warn!("Failed to serialize local response: {}", e),
        }
    }

    /// Answer a tool-on request (#166).
    ///
    /// The single way this topic is published, so `tool_id` cannot be left off
    /// by one of the three decision paths. A module needs it to tell its own
    /// answer from the answer to the reader at the next machine -- these topics
    /// carry no device id and are not namespaced, so without it every reply is
    /// addressed to everyone.
    fn publish_tool_on(
        &self,
        tool_id: &str,
        authorized: bool,
        reason: &str,
        strike_ms: Option<i32>,
    ) {
        let payload = LocalToolOnResponse {
            tool_id: tool_id.to_string(),
            authorized,
            reason: reason.to_string(),
        };
        match serde_json::to_value(&payload) {
            Ok(v) => self.publish_local(local::TOOL_ON_RESPONSE, &v),
            Err(e) => warn!("Failed to serialize tool-on response: {}", e),
        }
        // The same answer, resource-shaped (#182) -- with the pulse when the
        // resource has a latch, which the alias cannot say.
        let mut reply = LocalScanReply::from_tool(&payload);
        if authorized {
            reply.strike = strike_ms.map(|pulse_ms| css_lib::wire::LocalStrike { pulse_ms });
        }
        match serde_json::to_value(reply) {
            Ok(v) => self.publish_local(local::SCAN_RESPONSE, &v),
            Err(e) => warn!("Failed to serialize scan reply: {}", e),
        }
    }

    /// Start a task that periodically fetches calendar events from the remote
    /// server and publishes them as JSON to `calendar_topic` on the local broker.
    /// The first publish happens immediately on task start.
    pub fn start_calendar_publisher_task(&self, calendar_topic: String, interval_secs: u64) {
        let client = self.client.clone();
        let http_client = self.http_client.clone();
        let instance_url = self.remote_instance_url.clone();
        let auth_token = self.remote_auth_token.clone();
        let refresh_notify = self.refresh_notify.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {},
                    _ = refresh_notify.notified() => {},
                }

                let url = format!("{}/api/calendar/events", instance_url);
                match http_client.get(&url).bearer_auth(&auth_token).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        match resp
                            .json::<Vec<crate::calendar::ServerCalendarEvent>>()
                            .await
                        {
                            Ok(server_events) => {
                                let cal_events: Vec<crate::calendar::CalEvent> = server_events
                                    .into_iter()
                                    .map(crate::calendar::to_cal_event)
                                    .collect();
                                match serde_json::to_vec(&cal_events) {
                                    Ok(bytes) => {
                                        let msg = mqtt::Message::new(&calendar_topic, bytes, 1);
                                        if let Err(e) = client.publish(msg).wait() {
                                            warn!(
                                                "Failed to publish calendar events locally: {}",
                                                e
                                            );
                                        } else {
                                            info!(
                                                "Published {} calendar event(s) to {}",
                                                cal_events.len(),
                                                calendar_topic
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to serialize calendar events: {}", e)
                                    }
                                }
                            }
                            Err(e) => warn!("Failed to parse calendar events response: {}", e),
                        }
                    }
                    Ok(resp) => warn!("Calendar events request returned HTTP {}", resp.status()),
                    Err(e) => warn!("Failed to fetch calendar events from server: {}", e),
                }
            }
        });

        info!(
            "Calendar publisher task started (interval: {}s)",
            interval_secs
        );
    }

    pub fn disconnect(&self) -> Result<()> {
        self.client
            .disconnect(None)
            .wait()
            .context("Failed to disconnect local MQTT client")?;
        Ok(())
    }
}

/// Run the local MQTT message receiver loop.
/// `state_rx` receives a copy of the sync payload every time the edge's
/// toolguard state changes; the loop immediately publishes it to the local
/// broker on `toolguard/state` so subscribers are notified without polling.
pub async fn run_local_mqtt_event_loop(
    rx: mqtt::Receiver<Option<mqtt::Message>>,
    local_client: Arc<LocalMqttClient>,
    state_rx: std::sync::mpsc::Receiver<crate::toolguard::SyncPayload>,
) -> Result<()> {
    info!("Starting local MQTT event loop");

    let receiver_task = tokio::task::spawn_blocking(move || {
        loop {
            // Publish any pending state updates first
            while let Ok(payload) = state_rx.try_recv() {
                local_client.reconcile_sessions(&payload);
                match serde_json::to_vec(&payload) {
                    Ok(bytes) => local_client.publish_state_bytes(bytes),
                    Err(e) => warn!(
                        "Failed to serialize toolguard state for local publish: {}",
                        e
                    ),
                }
            }

            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(msg)) => {
                    let client = local_client.clone();
                    let topic = msg.topic().to_string();
                    let payload = msg.payload().to_vec();
                    tokio::spawn(async move {
                        client.handle_message(&topic, &payload).await;
                    });
                }
                Ok(None) => {
                    warn!("Local MQTT connection lost, waiting for reconnection...");
                    std::thread::sleep(Duration::from_secs(5));
                }
                Err(e) => {
                    if e.is_timeout() {
                        continue;
                    } else {
                        error!("Local MQTT receiver disconnected");
                        break;
                    }
                }
            }
        }
    });

    match receiver_task.await {
        Ok(_) => info!("Local MQTT receiver task completed"),
        Err(e) => error!("Local MQTT receiver task panicked: {}", e),
    }

    Ok(())
}
