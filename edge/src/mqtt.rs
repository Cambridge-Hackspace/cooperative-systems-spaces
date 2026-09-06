use anyhow::{Context, Result};
use paho_mqtt as mqtt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::config::{Config, MqttConfig};
use crate::doors::{
    self, Decision, DoorsEvent, DoorsState, LocalScanRequest, LocalUnlockResponse, UnlockCommand,
};
use crate::edge_inbound::EdgeInbound;
use crate::registration::{get_auth_token, get_device_id};
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

/// Local MQTT request topics the edge subscribes to
const LOCAL_TOOL_ON_REQ: &str = "toolguard/request/tool-on";
const LOCAL_TOOL_OFF_REQ: &str = "toolguard/request/tool-off";
const LOCAL_TOOL_LOG_REQ: &str = "toolguard/request/tool-log";
const LOCAL_KIOSK_REFRESH: &str = "kiosk/refresh";
/// RFID scan from the local hardware bridge — `{ door_id, card_id }`.
const LOCAL_DOOR_SCAN_REQ: &str = "door/request/scan";
/// Unlock response sent back to the local relay controller.
const LOCAL_DOOR_UNLOCK_RESP: &str = "door/response/unlock";

/// JSON published by local hardware onto the local broker
#[derive(Debug, Deserialize)]
struct LocalToolRequest {
    card: String,
    tool_id: String,
    #[serde(default)]
    seconds: Option<f32>,
    #[serde(default)]
    temperature: Option<f32>,
    /// The tool's own API key. Metered tools require it server-side (the global
    /// key is rejected for them); the controller supplies it and the edge
    /// forwards it. Absent for non-metered tools, which authenticate by the
    /// edge's device token.
    #[serde(default)]
    api_key: Option<String>,
}

pub struct LocalMqttClient {
    client: mqtt::AsyncClient,
    toolguard_state: Arc<ToolGuardState>,
    remote_instance_url: String,
    remote_auth_token: String,
    http_client: Client,
    refresh_notify: Arc<tokio::sync::Notify>,
    /// Shared decision cache populated by remote `doors/state` snapshots.
    doors_state: Arc<DoorsState>,
    /// Forwards local scan outcomes back to the server as `doors/event`. The
    /// receiver is consumed by a task in `main`.
    doors_event_tx: DoorsEventSender,
}

impl LocalMqttClient {
    pub async fn new(
        mqtt_config: &MqttConfig,
        toolguard_state: Arc<ToolGuardState>,
        remote_instance_url: String,
        remote_auth_token: String,
        doors_state: Arc<DoorsState>,
        doors_event_tx: DoorsEventSender,
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
                remote_instance_url,
                remote_auth_token,
                http_client: Client::new(),
                refresh_notify: Arc::new(tokio::sync::Notify::new()),
                doors_state,
                doors_event_tx,
            },
            rx,
        ))
    }

    pub fn subscribe_to_requests(&self) -> Result<()> {
        self.client
            .subscribe(LOCAL_TOOL_ON_REQ, 1)
            .wait()
            .context("Failed to subscribe to tool-on requests")?;
        self.client
            .subscribe(LOCAL_TOOL_OFF_REQ, 1)
            .wait()
            .context("Failed to subscribe to tool-off requests")?;
        self.client
            .subscribe(LOCAL_TOOL_LOG_REQ, 1)
            .wait()
            .context("Failed to subscribe to tool-log requests")?;
        self.client
            .subscribe(LOCAL_KIOSK_REFRESH, 0)
            .wait()
            .context("Failed to subscribe to kiosk refresh topic")?;
        self.client
            .subscribe(LOCAL_DOOR_SCAN_REQ, 1)
            .wait()
            .context("Failed to subscribe to door scan requests")?;
        info!("Subscribed to local toolguard + door request topics");
        Ok(())
    }

    pub async fn handle_message(&self, topic: &str, payload: &[u8]) {
        if topic == LOCAL_KIOSK_REFRESH {
            self.handle_refresh_request().await;
            return;
        }

        if topic == LOCAL_DOOR_SCAN_REQ {
            self.handle_door_scan(payload).await;
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
            LOCAL_TOOL_ON_REQ => self.handle_tool_on(req).await,
            LOCAL_TOOL_OFF_REQ => self.handle_tool_off(req).await,
            LOCAL_TOOL_LOG_REQ => self.handle_tool_log(req).await,
            _ => {}
        }
    }

    /// Decide an RFID scan against the local cache, publish a response back
    /// to the hardware, and forward the outcome to the server as `doors/event`.
    async fn handle_door_scan(&self, payload: &[u8]) {
        let req: LocalScanRequest = match serde_json::from_slice(payload) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to parse door scan request: {}", e);
                return;
            }
        };

        let (granted, duration_ms, reason) =
            match self.doors_state.decide(req.door_id, &req.card_id) {
                Decision::Allow { duration_ms } => (true, duration_ms, None),
                Decision::Deny(why) => (false, 0, Some(why.to_string())),
            };

        // Tell the local relay controller what to do.
        let response = LocalUnlockResponse {
            door_id: req.door_id,
            granted,
            duration_ms,
            reason: reason.clone(),
        };
        if let Ok(v) = serde_json::to_value(&response) {
            self.publish_local(LOCAL_DOOR_UNLOCK_RESP, &v);
        }

        // Tell the server what just happened (audit log, webhook, etc.).
        let event = doors::DoorsEvent {
            door_id: req.door_id,
            card_id: Some(req.card_id),
            granted,
            reason,
            source: "rfid",
            occurred_at: chrono::Utc::now(),
        };
        if self.doors_event_tx.send(event).is_err() {
            warn!("Doors event bridge channel closed; cannot report scan upstream");
        }
    }

    /// Forward a server-initiated unlock command onto the local broker so
    /// the relay actuates. Called by a bridge task in `main` that drains the
    /// channel populated by [`EdgeMqttClient`].
    pub fn publish_doors_unlock_response(&self, cmd: &UnlockCommand) {
        let response = LocalUnlockResponse {
            door_id: cmd.door_id,
            granted: true,
            duration_ms: cmd.duration_ms,
            reason: Some(cmd.reason.clone()),
        };
        if let Ok(v) = serde_json::to_value(&response) {
            self.publish_local(LOCAL_DOOR_UNLOCK_RESP, &v);
        }
    }

    /// Publish one Open Access hold pulse: a momentary unlock the hold-refresh
    /// loop re-sends before it elapses to keep the strike energized for the whole
    /// window. Same wire message as any unlock (the external relay firmware only
    /// understands the momentary `duration_ms`); holding is a property of the
    /// loop re-sending, and stopping is what relocks the door — fail-secure.
    pub fn publish_doors_hold(&self, door_id: uuid::Uuid, duration_ms: i32) {
        let response = LocalUnlockResponse {
            door_id,
            granted: true,
            duration_ms,
            reason: Some("open_access".to_string()),
        };
        if let Ok(v) = serde_json::to_value(&response) {
            self.publish_local(LOCAL_DOOR_UNLOCK_RESP, &v);
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

    async fn handle_tool_on(&self, req: LocalToolRequest) {
        use crate::toolguard::AccessResult;

        // Metered tools under online-synchronous actuation: the server is the
        // authority. Ask it (and place the hold) BEFORE energizing; if it is
        // unreachable, deny -- fail closed, because we cannot safely bill offline.
        if self.toolguard_state.tool_requires_online(&req.tool_id) {
            let (authorized, reason) = self.remote_tool_on(&req).await;
            let response_payload =
                serde_json::json!({ "authorized": authorized, "reason": reason });
            self.publish_local("toolguard/response/tool-on", &response_payload);
            return;
        }

        // Otherwise decide locally from the cached allow-list (offline-capable),
        // then best-effort forward so the server records it (and, for metered
        // tools in edge-local mode, places the hold and re-broadcasts).
        let result = self.toolguard_state.check_access(&req.card, &req.tool_id);

        let (authorized, reason) = match &result {
            AccessResult::Authorized => (true, "authorized".to_string()),
            AccessResult::UnknownCard => (false, "Unknown card".to_string()),
            AccessResult::UserInactive => (false, "User is not active".to_string()),
            AccessResult::ToolNotAuthorized => {
                (false, "Tool not authorized for this user".to_string())
            }
            AccessResult::ToolUnavailable(s) => (false, format!("Tool unavailable: {}", s)),
        };

        let response_payload = serde_json::json!({ "authorized": authorized, "reason": reason });
        self.publish_local("toolguard/response/tool-on", &response_payload);

        if authorized {
            // Best-effort forward to the remote server so it records the state change
            let url = format!("{}/api/toolguard/tool-on", self.remote_instance_url);
            let card = req.card.clone();
            let tool_id = req.tool_id.clone();
            let api_key = req.api_key.clone();
            let token = self.remote_auth_token.clone();
            let http = self.http_client.clone();
            tokio::spawn(async move {
                let mut params = vec![("card", card), ("tool_id", tool_id)];
                if let Some(k) = api_key {
                    params.push(("api_key", k));
                }
                if let Err(e) = http
                    .get(&url)
                    .bearer_auth(&token)
                    .query(&params)
                    .send()
                    .await
                {
                    warn!("Failed to forward tool-on to remote: {}", e);
                }
            });
        }
    }

    /// Synchronously ask the server to authorize (and hold) a metered activation.
    /// Returns `(authorized, reason)`. Any transport or non-2xx error is a denial
    /// -- fail closed.
    async fn remote_tool_on(&self, req: &LocalToolRequest) -> (bool, String) {
        let url = format!("{}/api/toolguard/tool-on", self.remote_instance_url);
        let mut params = vec![("card", req.card.clone()), ("tool_id", req.tool_id.clone())];
        if let Some(k) = &req.api_key {
            params.push(("api_key", k.clone()));
        }
        match self
            .http_client
            .get(&url)
            .bearer_auth(&self.remote_auth_token)
            .query(&params)
            .send()
            .await
        {
            Ok(resp) => match resp.json::<serde_json::Value>().await {
                Ok(body) => {
                    // The server's ToolGuardResponse for tool-on is
                    // { status, message, tool_on }: tool_on == true means
                    // authorized, and message carries the denial reason.
                    let authorized = body
                        .get("tool_on")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let reason = body
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(if authorized { "authorized" } else { "denied" })
                        .to_string();
                    (authorized, reason)
                }
                Err(e) => (false, format!("server response unreadable: {e}")),
            },
            Err(e) => {
                warn!("online-sync tool-on to remote failed: {}", e);
                (false, "server unreachable".to_string())
            }
        }
    }

    async fn handle_tool_off(&self, req: LocalToolRequest) {
        let response_payload = serde_json::json!({ "ok": true });
        self.publish_local("toolguard/response/tool-off", &response_payload);

        let url = format!("{}/api/toolguard/tool-off", self.remote_instance_url);
        let card = req.card.clone();
        let tool_id = req.tool_id.clone();
        let api_key = req.api_key.clone();
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        tokio::spawn(async move {
            let mut params = vec![("card", card), ("tool_id", tool_id)];
            if let Some(k) = api_key {
                params.push(("api_key", k));
            }
            if let Err(e) = http
                .get(&url)
                .bearer_auth(&token)
                .query(&params)
                .send()
                .await
            {
                warn!("Failed to forward tool-off to remote: {}", e);
            }
        });
    }

    async fn handle_tool_log(&self, req: LocalToolRequest) {
        let seconds = req.seconds.unwrap_or(0.0);
        let response_payload = serde_json::json!({ "ok": true });
        self.publish_local("toolguard/response/tool-log", &response_payload);

        let url = format!("{}/api/toolguard/tool-log", self.remote_instance_url);
        let token = self.remote_auth_token.clone();
        let http = self.http_client.clone();
        let card = req.card.clone();
        let tool_id = req.tool_id.clone();
        let temperature = req.temperature;
        let api_key = req.api_key.clone();
        tokio::spawn(async move {
            let mut params = vec![
                ("card", card),
                ("tool_id", tool_id),
                ("seconds", seconds.to_string()),
            ];
            if let Some(t) = temperature {
                params.push(("temperature", t.to_string()));
            }
            if let Some(k) = api_key {
                params.push(("api_key", k));
            }
            if let Err(e) = http
                .get(&url)
                .bearer_auth(&token)
                .query(&params)
                .send()
                .await
            {
                warn!("Failed to forward tool-log to remote: {}", e);
            }
        });
    }

    /// Publish the current toolguard state to the local broker so subscribers
    /// (e.g. status kiosks) receive it immediately.
    pub fn publish_state_bytes(&self, bytes: Vec<u8>) {
        let msg = mqtt::Message::new("toolguard/state", bytes, 1);
        if let Err(e) = self.client.publish(msg).wait() {
            warn!("Failed to publish toolguard state to local broker: {}", e);
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
