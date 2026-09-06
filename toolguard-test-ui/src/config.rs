use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCard {
    /// Display name shown in the UI
    pub name: String,
    /// The value that matches the toolguard profile_field (e.g. card_id)
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualTool {
    /// Display name shown in the UI
    pub name: String,
    /// The external_id sent as `tool_id` in MQTT messages
    pub id: String,
    /// The tool's own `external_api_key`. A metered tool requires it: the server
    /// accepts only the per-tool key (not the shared global key) on the money
    /// path, so a metered tool driven without it is refused. Left unset for
    /// free/non-metered tools, which authenticate via the edge's device token.
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Local MQTT broker URL (e.g. "tcp://localhost:1883")
    pub mqtt_broker_url: String,
    /// MQTT client ID for this test UI
    #[serde(default = "default_client_id")]
    pub mqtt_client_id: String,
    /// Virtual cards available for scanning
    pub cards: Vec<VirtualCard>,
    /// Virtual tools to control
    pub tools: Vec<VirtualTool>,
}

fn default_client_id() -> String {
    "toolguard-test-ui".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mqtt_broker_url: "tcp://localhost:1883".to_string(),
            mqtt_client_id: default_client_id(),
            cards: vec![
                VirtualCard {
                    name: "Alice".to_string(),
                    value: "ALICE001".to_string(),
                },
                VirtualCard {
                    name: "Bob".to_string(),
                    value: "BOB002".to_string(),
                },
            ],
            tools: vec![
                VirtualTool {
                    name: "Laser Cutter".to_string(),
                    id: "laser-01".to_string(),
                    // A metered tool: give it its own key so the sample config
                    // can drive the money path. Must match the tool's
                    // external_api_key on the server.
                    api_key: Some("laser-01-key".to_string()),
                },
                VirtualTool {
                    name: "3D Printer".to_string(),
                    id: "3dprinter-01".to_string(),
                    api_key: None,
                },
            ],
        }
    }
}

impl Config {
    pub fn load_or_default(path: &str) -> Self {
        let p = Path::new(path);
        if p.exists() {
            match fs::read_to_string(p) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(cfg) => return cfg,
                    Err(e) => eprintln!("Config parse error: {e} — using defaults"),
                },
                Err(e) => eprintln!("Config read error: {e} — using defaults"),
            }
        } else {
            // Write a sample config so the user can edit it
            let sample = toml::to_string_pretty(&Config::default()).unwrap_or_default();
            let _ = fs::write(p, sample);
            eprintln!("No config found — wrote defaults to {path}");
        }
        Config::default()
    }
}
