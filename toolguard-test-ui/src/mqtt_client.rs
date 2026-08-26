use paho_mqtt as mqtt;
use serde::Deserialize;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

// ── Commands sent from the UI to the MQTT thread ─────────────────────────────

#[derive(Debug)]
pub enum MqttCommand {
    ToolOn { card: String, tool_id: String },
    ToolOff { card: String, tool_id: String },
}

// ── Events sent from the MQTT thread back to the UI ──────────────────────────

#[derive(Debug, Clone)]
pub enum MqttEvent {
    Connected,
    Disconnected,
    ToolOnResponse {
        tool_id: String,
        authorized: bool,
        reason: String,
    },
    ToolOffResponse {
        tool_id: String,
        ok: bool,
    },
    /// Live status update from `toolguard/state`: (tool_id, status_str) pairs
    StateUpdate(Vec<(String, String)>),
    Error(String),
}

// ── Deserialization helpers ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct ToolOnPayload {
    authorized: bool,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct ToolOffPayload {
    ok: bool,
}

// SyncPayload mirrors the edge toolguard/state topic shape
#[derive(Deserialize)]
struct SyncTool {
    #[serde(default)]
    external_id: Option<String>,
    id: String,
    status: String,
}

#[derive(Deserialize)]
struct SyncPayload {
    tools: Vec<SyncTool>,
}

// ── Response topics ───────────────────────────────────────────────────────────

const TOOL_ON_RESP: &str = "toolguard/response/tool-on";
const TOOL_OFF_RESP: &str = "toolguard/response/tool-off";
const STATE_TOPIC: &str = "toolguard/state";

// ── Public API ────────────────────────────────────────────────────────────────

/// Spawn the MQTT background thread.  Returns a command sender and an event receiver.
pub fn spawn(
    broker_url: String,
    client_id: String,
    startup_tool_offs: Vec<(String, String)>, // (card, tool_id) pairs
) -> (Sender<MqttCommand>, Receiver<MqttEvent>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<MqttCommand>();
    let (evt_tx, evt_rx) = std::sync::mpsc::channel::<MqttEvent>();

    std::thread::spawn(move || {
        run_loop(broker_url, client_id, startup_tool_offs, cmd_rx, evt_tx);
    });

    (cmd_tx, evt_rx)
}

// ── Thread body ───────────────────────────────────────────────────────────────

fn run_loop(
    broker_url: String,
    client_id: String,
    startup_tool_offs: Vec<(String, String)>,
    cmd_rx: Receiver<MqttCommand>,
    evt_tx: Sender<MqttEvent>,
) {
    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(&broker_url)
        .client_id(&client_id)
        .finalize();

    let client = match mqtt::Client::new(create_opts) {
        Ok(c) => c,
        Err(e) => {
            let _ = evt_tx.send(MqttEvent::Error(format!(
                "Failed to create MQTT client: {e}"
            )));
            return;
        }
    };

    let conn_opts = mqtt::ConnectOptionsBuilder::new()
        .keep_alive_interval(Duration::from_secs(30))
        .clean_session(true)
        .automatic_reconnect(Duration::from_secs(1), Duration::from_secs(30))
        .finalize();

    if let Err(e) = client.connect(conn_opts) {
        let _ = evt_tx.send(MqttEvent::Error(format!("MQTT connect failed: {e}")));
        return;
    }

    let _ = evt_tx.send(MqttEvent::Connected);

    // Subscribe to response topics and live state
    let _ = client.subscribe(TOOL_ON_RESP, 1);
    let _ = client.subscribe(TOOL_OFF_RESP, 1);
    let _ = client.subscribe(STATE_TOPIC, 1);

    // Startup: publish tool-off for all known tools
    for (card, tool_id) in startup_tool_offs {
        publish_tool_off(&client, &card, &tool_id);
    }

    // Set up the receiver consumer
    let receiver = client.start_consuming();

    loop {
        // Drain any incoming MQTT messages
        loop {
            match receiver.try_recv() {
                Ok(Some(msg)) => handle_incoming(&evt_tx, &msg),
                Ok(None) => {
                    // Connection dropped
                    let _ = evt_tx.send(MqttEvent::Disconnected);
                    // Try to reconnect
                    if let Err(e) = client.reconnect() {
                        let _ = evt_tx.send(MqttEvent::Error(format!("Reconnect failed: {e}")));
                    } else {
                        let _ = evt_tx.send(MqttEvent::Connected);
                    }
                    break;
                }
                Err(_) => break, // Channel empty
            }
        }

        // Drain any UI commands (non-blocking)
        loop {
            match cmd_rx.try_recv() {
                Ok(MqttCommand::ToolOn { card, tool_id }) => {
                    publish_tool_on(&client, &card, &tool_id)
                }
                Ok(MqttCommand::ToolOff { card, tool_id }) => {
                    publish_tool_off(&client, &card, &tool_id)
                }
                Err(_) => break,
            }
        }

        std::thread::sleep(Duration::from_millis(20));
    }
}

fn publish_tool_on(client: &mqtt::Client, card: &str, tool_id: &str) {
    let payload = serde_json::json!({ "card": card, "tool_id": tool_id }).to_string();
    let msg = mqtt::Message::new("toolguard/request/tool-on", payload, 1);
    if let Err(e) = client.publish(msg) {
        eprintln!("Failed to publish tool-on: {e}");
    }
}

fn publish_tool_off(client: &mqtt::Client, card: &str, tool_id: &str) {
    let payload = serde_json::json!({ "card": card, "tool_id": tool_id }).to_string();
    let msg = mqtt::Message::new("toolguard/request/tool-off", payload, 1);
    if let Err(e) = client.publish(msg) {
        eprintln!("Failed to publish tool-off: {e}");
    }
}

fn handle_incoming(evt_tx: &Sender<MqttEvent>, msg: &mqtt::Message) {
    let topic = msg.topic();
    let payload = msg.payload();

    if topic == TOOL_ON_RESP {
        match serde_json::from_slice::<ToolOnPayload>(payload) {
            Ok(p) => {
                let _ = evt_tx.send(MqttEvent::ToolOnResponse {
                    tool_id: String::new(), // matched by App via pending_scan
                    authorized: p.authorized,
                    reason: p.reason,
                });
            }
            Err(e) => {
                let _ = evt_tx.send(MqttEvent::Error(format!("Bad tool-on response: {e}")));
            }
        }
    } else if topic == TOOL_OFF_RESP {
        match serde_json::from_slice::<ToolOffPayload>(payload) {
            Ok(p) => {
                let _ = evt_tx.send(MqttEvent::ToolOffResponse {
                    tool_id: String::new(),
                    ok: p.ok,
                });
            }
            Err(e) => {
                let _ = evt_tx.send(MqttEvent::Error(format!("Bad tool-off response: {e}")));
            }
        }
    } else if topic == STATE_TOPIC {
        match serde_json::from_slice::<SyncPayload>(payload) {
            Ok(sync) => {
                // Index by both external_id and uuid id so lookup works regardless
                let mut seen = std::collections::HashMap::<String, String>::new();
                for tool in &sync.tools {
                    let status = serde_json::to_string(&tool.status)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string();
                    let key = tool.external_id.clone().unwrap_or_else(|| tool.id.clone());
                    seen.entry(key).or_insert_with(|| status.clone());
                    seen.entry(tool.id.clone()).or_insert_with(|| status);
                }
                let statuses = seen.into_iter().collect();
                let _ = evt_tx.send(MqttEvent::StateUpdate(statuses));
            }
            Err(e) => {
                let _ = evt_tx.send(MqttEvent::Error(format!("Bad state payload: {e}")));
            }
        }
    }
}
