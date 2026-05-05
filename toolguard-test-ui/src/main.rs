mod config;
mod mqtt_client;

use config::{Config, VirtualCard};
use egui::{Color32, RichText, Sense, Vec2};
use mqtt_client::{MqttCommand, MqttEvent};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};

const CONFIG_PATH: &str = "toolguard-test-ui.toml";
const LOG_MAX: usize = 50;

// ── Tool status (mirrors server ToolStatus enum) ──────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ToolStatus {
    Unknown,      // Before any state arrives
    Idle,
    InUse,
    Maintenance,
    Repair,
    Broken,
    Retired,
}

impl ToolStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "idle"        => Self::Idle,
            "in_use"      => Self::InUse,
            "maintenance" => Self::Maintenance,
            "repair"      => Self::Repair,
            "broken"      => Self::Broken,
            "retired"     => Self::Retired,
            _             => Self::Unknown,
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Unknown     => "Unknown",
            Self::Idle        => "Idle",
            Self::InUse       => "In Use",
            Self::Maintenance => "Maintenance",
            Self::Repair      => "Repair",
            Self::Broken      => "Broken",
            Self::Retired     => "Retired",
        }
    }

    fn dot_color(&self) -> Color32 {
        match self {
            Self::Unknown     => Color32::from_rgb(100, 100, 100),
            Self::Idle        => Color32::from_rgb(50, 200, 80),
            Self::InUse       => Color32::from_rgb(50, 150, 230),
            Self::Maintenance => Color32::from_rgb(230, 190, 40),
            Self::Repair      => Color32::from_rgb(230, 130, 40),
            Self::Broken      => Color32::from_rgb(220, 60, 60),
            Self::Retired     => Color32::from_rgb(120, 120, 120),
        }
    }

    fn frame_color(&self) -> Color32 {
        match self {
            Self::Unknown     => Color32::from_rgb(45, 45, 45),
            Self::Idle        => Color32::from_rgb(30, 60, 35),
            Self::InUse       => Color32::from_rgb(25, 45, 75),
            Self::Maintenance => Color32::from_rgb(65, 55, 15),
            Self::Repair      => Color32::from_rgb(65, 40, 15),
            Self::Broken      => Color32::from_rgb(70, 25, 25),
            Self::Retired     => Color32::from_rgb(40, 40, 40),
        }
    }

    /// Whether a scan (tool-on/off toggle) is permitted in this state
    fn can_scan(&self) -> bool {
        matches!(self, Self::Idle | Self::InUse)
    }
}

// ── Per-tool runtime state ────────────────────────────────────────────────────

#[derive(Debug)]
struct ToolStation {
    name: String,
    id: String,
    /// Server-reported tool status
    status: ToolStatus,
    /// Index into App::cards for the currently selected card
    selected_card: usize,
    /// Last response message received for this tool
    last_response: Option<String>,
    /// Waiting for a response (shows spinner-like pending state)
    pending: bool,
}

// ── Application ───────────────────────────────────────────────────────────────

struct App {
    cards: Vec<VirtualCard>,
    tools: Vec<ToolStation>,

    cmd_tx: Sender<MqttCommand>,
    evt_rx: Receiver<MqttEvent>,

    mqtt_connected: bool,
    /// Pending scan: (tool_id_str, was_on_before_scan) — used to match async response
    pending_scan: Option<(String, bool)>,
    /// Activity log
    log: VecDeque<String>,
}

impl App {
    fn new(config: Config) -> Self {
        let startup_tool_offs: Vec<(String, String)> = if config.cards.is_empty() {
            vec![]
        } else {
            let first_card = config.cards[0].value.clone();
            config.tools.iter()
                .map(|t| (first_card.clone(), t.id.clone()))
                .collect()
        };

        let (cmd_tx, evt_rx) = mqtt_client::spawn(
            config.mqtt_broker_url.clone(),
            config.mqtt_client_id.clone(),
            startup_tool_offs,
        );

        let tools = config.tools.iter().map(|t| ToolStation {
            name: t.name.clone(),
            id: t.id.clone(),
            status: ToolStatus::Unknown,
            selected_card: 0,
            last_response: None,
            pending: false,
        }).collect();

        Self {
            cards: config.cards,
            tools,
            cmd_tx,
            evt_rx,
            mqtt_connected: false,
            pending_scan: None,
            log: VecDeque::new(),
        }
    }

    fn push_log(&mut self, msg: impl Into<String>) {
        let entry = msg.into();
        if self.log.len() >= LOG_MAX {
            self.log.pop_front();
        }
        self.log.push_back(entry);
    }

    /// Drain the MQTT event channel and update state accordingly.
    fn poll_mqtt(&mut self) {
        while let Ok(event) = self.evt_rx.try_recv() {
            match event {
                MqttEvent::Connected => {
                    self.mqtt_connected = true;
                    self.push_log("MQTT connected");
                }
                MqttEvent::Disconnected => {
                    self.mqtt_connected = false;
                    self.push_log("MQTT disconnected");
                }
                MqttEvent::ToolOnResponse { authorized, reason, .. } => {
                    if let Some((tool_id, _)) = self.pending_scan.take() {
                        if let Some(station) = self.tools.iter_mut().find(|t| t.id == tool_id) {
                            station.pending = false;
                            if authorized {
                                station.status = ToolStatus::InUse;
                                station.last_response = Some("✓ Authorized".to_string());
                                self.log.push_back(format!("{}: authorized", station.name));
                            } else {
                                let msg = if reason.is_empty() { "Denied".to_string() } else { reason.clone() };
                                station.last_response = Some(format!("✗ {msg}"));
                                self.log.push_back(format!("{}: denied — {msg}", station.name));
                            }
                        }
                    }
                }
                MqttEvent::ToolOffResponse { ok, .. } => {
                    if let Some((tool_id, _)) = self.pending_scan.take() {
                        if let Some(station) = self.tools.iter_mut().find(|t| t.id == tool_id) {
                            station.pending = false;
                            if ok {
                                station.status = ToolStatus::Idle;
                                station.last_response = Some("✓ Deactivated".to_string());
                            } else {
                                station.last_response = Some("✗ Deactivate failed".to_string());
                            }
                            self.log.push_back(format!("{}: deactivated (ok={})", station.name, ok));
                        }
                    }
                }
                MqttEvent::StateUpdate(statuses) => {
                    for station in &mut self.tools {
                        if let Some((_, status_str)) = statuses.iter().find(|(id, _)| id == &station.id) {
                            let new_status = ToolStatus::from_str(status_str);
                            if new_status != station.status {
                                self.log.push_back(format!(
                                    "{}: {} → {}",
                                    station.name, station.status.label(), new_status.label()
                                ));
                                station.status = new_status;
                            }
                        }
                    }
                }
                MqttEvent::Error(e) => {
                    self.push_log(format!("Error: {e}"));
                }
            }
        }
        // Keep log bounded (may have been pushed directly)
        while self.log.len() > LOG_MAX {
            self.log.pop_front();
        }
    }

    fn scan(&mut self, tool_idx: usize) {
        let Some(station) = self.tools.get(tool_idx) else { return };
        let Some(card) = self.cards.get(station.selected_card) else { return };

        let is_inuse = station.status == ToolStatus::InUse;
        let tool_id = station.id.clone();
        let card_value = card.value.clone();
        let card_name = card.name.clone();
        let tool_name = station.name.clone();

        self.push_log(format!(
            "Scan: card='{}' → '{}' ({})",
            card_name, tool_name, if is_inuse { "off" } else { "on" }
        ));

        let cmd = if is_inuse {
            MqttCommand::ToolOff { card: card_value, tool_id: tool_id.clone() }
        } else {
            MqttCommand::ToolOn  { card: card_value, tool_id: tool_id.clone() }
        };

        let _ = self.cmd_tx.send(cmd);

        self.pending_scan = Some((tool_id.clone(), is_inuse));
        if let Some(station) = self.tools.get_mut(tool_idx) {
            station.pending = true;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_mqtt();
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        // ── Top bar ───────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("ToolGuard Test UI");
                ui.separator();
                let (dot_color, status_text) = if self.mqtt_connected {
                    (Color32::GREEN, "MQTT connected")
                } else {
                    (Color32::RED, "MQTT disconnected")
                };
                status_dot(ui, dot_color);
                ui.label(status_text);
            });
        });

        // ── Log panel ─────────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("log_panel")
            .resizable(true)
            .min_height(80.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Activity Log").small().weak());
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for entry in &self.log {
                            ui.label(RichText::new(entry).monospace().small());
                        }
                    });
            });

        // ── Tool stations ─────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.cards.is_empty() {
                ui.label("No cards configured. Edit toolguard-test-ui.toml.");
                return;
            }

            ui.label(RichText::new("Tool Stations").strong());
            ui.add_space(4.0);

            let card_names: Vec<&str> = self.cards.iter().map(|c| c.name.as_str()).collect();

            // Collect scan requests first to avoid borrow issues
            let mut scan_idx: Option<usize> = None;

            for (i, station) in self.tools.iter_mut().enumerate() {
                let frame_color = if station.pending {
                    Color32::from_rgb(50, 50, 30) // neutral pending tint
                } else {
                    station.status.frame_color()
                };

                egui::Frame::none()
                    .fill(frame_color)
                    .rounding(6.0)
                    .inner_margin(12.0)
                    .outer_margin(egui::Margin::symmetric(0.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // State indicator dot
                            let dot_color = if station.pending {
                                Color32::YELLOW
                            } else {
                                station.status.dot_color()
                            };
                            status_dot(ui, dot_color);

                            // Tool name + state label
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&station.name).strong().size(16.0));
                                let state_text = if station.pending {
                                    RichText::new("Waiting…").italics().color(Color32::YELLOW)
                                } else {
                                    RichText::new(station.status.label())
                                        .strong()
                                        .color(station.status.dot_color())
                                };
                                ui.label(state_text);
                                if let Some(resp) = &station.last_response {
                                    ui.label(RichText::new(resp).small().weak());
                                }
                                ui.label(RichText::new(format!("id: {}", station.id)).small().weak());
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Scan button — only meaningful for Idle/InUse
                                let btn_label = if station.status == ToolStatus::InUse {
                                    "⏹  Stop"
                                } else {
                                    "▶  Start"
                                };
                                let enabled = !station.pending && station.status.can_scan();
                                let btn = egui::Button::new(btn_label)
                                    .min_size(Vec2::new(90.0, 36.0));
                                if ui.add_enabled(enabled, btn).clicked() {
                                    scan_idx = Some(i);
                                }

                                ui.add_space(8.0);

                                // Card selector
                                egui::ComboBox::from_id_salt(format!("card_{i}"))
                                    .selected_text(*card_names.get(station.selected_card).unwrap_or(&"—"))
                                    .width(120.0)
                                    .show_ui(ui, |ui| {
                                        for (ci, name) in card_names.iter().enumerate() {
                                            ui.selectable_value(&mut station.selected_card, ci, *name);
                                        }
                                    });
                                ui.label("Card:");
                            });
                        });
                    });
            }

            if let Some(idx) = scan_idx {
                self.scan(idx);
            }
        });
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let size = Vec2::splat(16.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().circle_filled(rect.center(), 7.0, color);
    ui.painter().circle_stroke(rect.center(), 7.0, egui::Stroke::new(1.0, Color32::from_black_alpha(80)));
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let config = Config::load_or_default(CONFIG_PATH);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ToolGuard Test UI")
            .with_inner_size([560.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ToolGuard Test UI",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(config)))),
    )
}
