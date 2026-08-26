use egui::{Color32, RichText, Sense, Vec2};
use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

const CONFIG_PATH: &str = "toolguard-status-ui.toml";
const STATE_TOPIC: &str = "toolguard/state";

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    /// Local MQTT broker URL (e.g. "tcp://localhost:1883")
    mqtt_broker_url: String,
    #[serde(default = "default_client_id")]
    mqtt_client_id: String,
}

fn default_client_id() -> String {
    "toolguard-status-ui".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mqtt_broker_url: "tcp://localhost:1883".to_string(),
            mqtt_client_id: default_client_id(),
        }
    }
}

impl Config {
    fn load_or_default(path: &str) -> Self {
        let p = std::path::Path::new(path);
        if p.exists() {
            if let Ok(content) = std::fs::read_to_string(p) {
                if let Ok(cfg) = toml::from_str(&content) {
                    return cfg;
                }
            }
        } else {
            let _ = std::fs::write(
                p,
                toml::to_string_pretty(&Config::default()).unwrap_or_default(),
            );
        }
        Config::default()
    }
}

// ── Sync payload (mirrors edge toolguard module) ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolStatus {
    Idle,
    InUse,
    Maintenance,
    Broken,
    Repair,
    Retired,
}

#[derive(Debug, Clone, Deserialize)]
struct SyncTool {
    id: String,
    name: String,
    status: ToolStatus,
}

#[derive(Debug, Clone, Deserialize)]
struct SyncUser {
    full_name: String,
    authorized_tool_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SyncPayload {
    tools: Vec<SyncTool>,
    users: Vec<SyncUser>,
}

// ── Derived display model ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ToolDisplay {
    name: String,
    status: ToolStatus,
    /// Populated when status == InUse
    inuse_by: Option<String>,
}

fn tools_from_payload(payload: &SyncPayload) -> Vec<ToolDisplay> {
    // Build a map from tool id → user full_name for inuse tools
    let mut inuse_by: HashMap<String, String> = HashMap::new();
    for user in &payload.users {
        for tool in &payload.tools {
            if tool.status == ToolStatus::InUse && user.authorized_tool_ids.contains(&tool.id) {
                inuse_by
                    .entry(tool.id.clone())
                    .or_insert_with(|| user.full_name.clone());
            }
        }
    }

    let mut tools: Vec<ToolDisplay> = payload
        .tools
        .iter()
        .map(|t| ToolDisplay {
            name: t.name.clone(),
            status: t.status.clone(),
            inuse_by: inuse_by.get(&t.id).cloned(),
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

fn status_color(status: &ToolStatus) -> Color32 {
    match status {
        ToolStatus::Idle => Color32::from_rgb(50, 200, 80),
        ToolStatus::InUse => Color32::from_rgb(50, 150, 230),
        ToolStatus::Maintenance => Color32::from_rgb(230, 190, 40),
        ToolStatus::Repair => Color32::from_rgb(230, 130, 40),
        ToolStatus::Broken => Color32::from_rgb(220, 60, 60),
        ToolStatus::Retired => Color32::from_rgb(120, 120, 120),
    }
}

fn status_label(status: &ToolStatus) -> &str {
    match status {
        ToolStatus::Idle => "Idle",
        ToolStatus::InUse => "In Use",
        ToolStatus::Maintenance => "Maintenance",
        ToolStatus::Repair => "Repair",
        ToolStatus::Broken => "Broken",
        ToolStatus::Retired => "Retired",
    }
}

// ── MQTT subscriber thread ────────────────────────────────────────────────────

#[derive(Debug)]
enum MqttEvent {
    Connected,
    Disconnected,
    State(Vec<ToolDisplay>),
    Error(String),
}

fn spawn_subscriber(broker_url: String, client_id: String) -> Receiver<MqttEvent> {
    let (tx, rx): (Sender<MqttEvent>, Receiver<MqttEvent>) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        run_subscriber(broker_url, client_id, tx);
    });
    rx
}

fn run_subscriber(broker_url: String, client_id: String, tx: Sender<MqttEvent>) {
    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(&broker_url)
        .client_id(&client_id)
        .finalize();

    let client = match mqtt::Client::new(create_opts) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(MqttEvent::Error(format!(
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
        let _ = tx.send(MqttEvent::Error(format!("MQTT connect failed: {e}")));
        return;
    }

    let _ = tx.send(MqttEvent::Connected);
    let _ = client.subscribe(STATE_TOPIC, 1);
    let receiver = client.start_consuming();

    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(msg)) if msg.topic() == STATE_TOPIC => {
                match serde_json::from_slice::<SyncPayload>(msg.payload()) {
                    Ok(payload) => {
                        let _ = tx.send(MqttEvent::State(tools_from_payload(&payload)));
                    }
                    Err(e) => {
                        let _ = tx.send(MqttEvent::Error(format!("Parse error: {e}")));
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                let _ = tx.send(MqttEvent::Disconnected);
                if let Err(e) = client.reconnect() {
                    let _ = tx.send(MqttEvent::Error(format!("Reconnect failed: {e}")));
                } else {
                    let _ = tx.send(MqttEvent::Connected);
                    let _ = client.subscribe(STATE_TOPIC, 1);
                }
            }
            Err(e) if e.is_timeout() => continue,
            Err(_) => break,
        }
    }
}

// ── Application ───────────────────────────────────────────────────────────────

struct App {
    tools: Vec<ToolDisplay>,
    connected: bool,
    last_error: Option<String>,
    rx: Receiver<MqttEvent>,
}

impl App {
    fn new(config: Config) -> Self {
        let rx = spawn_subscriber(
            config.mqtt_broker_url.clone(),
            config.mqtt_client_id.clone(),
        );
        Self {
            tools: vec![],
            connected: false,
            last_error: None,
            rx,
        }
    }

    fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                MqttEvent::Connected => {
                    self.connected = true;
                    self.last_error = None;
                }
                MqttEvent::Disconnected => {
                    self.connected = false;
                }
                MqttEvent::State(tools) => {
                    self.tools = tools;
                    self.last_error = None;
                }
                MqttEvent::Error(e) => {
                    self.last_error = Some(e);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        ctx.request_repaint_after(Duration::from_millis(100));

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Tool Status");
                ui.separator();
                let (color, label) = if self.connected {
                    (Color32::GREEN, "connected")
                } else {
                    (Color32::RED, "disconnected")
                };
                small_dot(ui, color);
                ui.label(label);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.last_error {
                ui.colored_label(Color32::RED, format!("⚠ {err}"));
                ui.separator();
            }

            if self.tools.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Waiting for state…").italics().weak());
                });
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                for tool in &self.tools {
                    let color = status_color(&tool.status);
                    egui::Frame::none()
                        .fill(Color32::from_gray(30))
                        .rounding(6.0)
                        .inner_margin(10.0)
                        .outer_margin(egui::Margin::symmetric(0.0, 3.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.set_min_height(44.0);
                            ui.horizontal(|ui| {
                                dot(ui, color);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&tool.name).strong().size(45.0));
                                    ui.label(
                                        RichText::new(status_label(&tool.status))
                                            .size(36.0)
                                            .color(color),
                                    );
                                    if let Some(user) = &tool.inuse_by {
                                        ui.label(
                                            RichText::new(user)
                                                .size(28.0)
                                                .color(Color32::from_rgb(180, 180, 180)),
                                        );
                                    }
                                });
                            });
                        });
                }
            });
        });
    }
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(120.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 54.0, color);
    ui.painter().circle_stroke(
        rect.center(),
        54.0,
        egui::Stroke::new(2.0, Color32::from_black_alpha(80)),
    );
}

fn small_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 9.0, color);
    ui.painter().circle_stroke(
        rect.center(),
        9.0,
        egui::Stroke::new(1.0, Color32::from_black_alpha(80)),
    );
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let config = Config::load_or_default(CONFIG_PATH);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ToolGuard Status")
            .with_inner_size([900.0, 1200.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ToolGuard Status",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(config)))),
    )
}
