use serde::{Deserialize, Serialize};

use kiosk::{KioskDisplayMode, KioskTheme};

const CONFIG_FILENAME: &str = "css-kiosk/kiosk.toml";

fn config_search_paths() -> Vec<std::path::PathBuf> {
    let mut paths = vec![std::path::PathBuf::from("kiosk.toml")];

    // $XDG_CONFIG_HOME/css-kiosk/kiosk.toml  (default: ~/.config)
    let xdg_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    if let Some(home) = xdg_home {
        paths.push(home.join(CONFIG_FILENAME));
    }

    // $XDG_CONFIG_DIRS/css-kiosk/kiosk.toml  (default: /etc/xdg)
    let xdg_dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_string());
    for dir in xdg_dirs.split(':').filter(|s| !s.is_empty()) {
        paths.push(std::path::PathBuf::from(dir).join(CONFIG_FILENAME));
    }

    paths
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KioskConfig {
    pub mqtt_broker_url: String,
    #[serde(default = "default_client_id")]
    pub mqtt_client_id: String,
    #[serde(default = "default_toolguard_topic")]
    pub toolguard_topic: String,
    /// Topic for calendar events. When set, the Calendar view is enabled.
    pub calendar_topic: Option<String>,
    /// How the kiosk displays and cycles its views.
    #[serde(default)]
    pub display_mode: KioskDisplayMode,
    /// Visual colour theme.
    #[serde(default)]
    pub theme: KioskTheme,
}

fn default_client_id() -> String {
    "kiosk".to_string()
}
fn default_toolguard_topic() -> String {
    "toolguard/state".to_string()
}

impl Default for KioskConfig {
    fn default() -> Self {
        Self {
            mqtt_broker_url: "tcp://localhost:1883".to_string(),
            mqtt_client_id: default_client_id(),
            toolguard_topic: default_toolguard_topic(),
            calendar_topic: None,
            display_mode: KioskDisplayMode::default(),
            theme: KioskTheme::default(),
        }
    }
}

impl KioskConfig {
    pub fn load_or_default() -> Self {
        for path in config_search_paths() {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(cfg) = toml::from_str(&content) {
                        return cfg;
                    }
                }
            }
        }
        // No config found — write a template to cwd for the operator to edit.
        let default = KioskConfig::default();
        let _ = std::fs::write(
            "kiosk.toml",
            toml::to_string_pretty(&default).unwrap_or_default(),
        );
        default
    }
}
