use std::fmt::{Display, Formatter};
use serde::{Deserialize, Serialize};

pub const MQTT_DEFAULT_NAMESPACE: &str = "cs/spaces";

/// MQTT Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    /// MQTT broker URL used by the CSS Instance
    pub mqtt_instance_url: String,

    /// MQTT Client ID used by the CSS Edge Client
    pub mqtt_client_id: String,

    /// MQTT username used by the CSS Edge Client
    pub mqtt_username: Option<String>,

    /// MQTT password used by the CSS Edge Client
    pub mqtt_password: Option<String>,

    /// MQTT Namespace used by the CSS Edge Client
    pub mqtt_namespace: String,
}

impl Display for MqttConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(write!(f, "MQTT: {}:{}", self.mqtt_instance_url, self.mqtt_client_id)?)
    }
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            mqtt_instance_url: "mqtt://localhost:1883".to_string(),
            mqtt_client_id: "css-server".to_string(),
            mqtt_username: None,
            mqtt_password: None,
            mqtt_namespace: MQTT_DEFAULT_NAMESPACE.to_string()

        }
    }
}

pub mod wire;

#[cfg(test)]
mod tests {
    use super::*;

    /// The namespace is a wire-visible constant: the server publishes to
    /// `{namespace}/devices/...` and the edge subscribes to the same prefix, so
    /// changing it is a protocol break rather than a rename. Pinned here so
    /// that break cannot happen silently.
    #[test]
    fn default_namespace_is_stable() {
        assert_eq!(MQTT_DEFAULT_NAMESPACE, "cs/spaces");
        assert_eq!(MqttConfig::default().mqtt_namespace, MQTT_DEFAULT_NAMESPACE);
    }

    #[test]
    fn config_round_trips_through_json() {
        let cfg = MqttConfig {
            mqtt_instance_url: "mqtt://broker.example:1883".to_string(),
            mqtt_client_id: "css-edge-1".to_string(),
            mqtt_username: Some("u".to_string()),
            mqtt_password: Some("p".to_string()),
            mqtt_namespace: "cs/spaces".to_string(),
        };
        let back: MqttConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.mqtt_instance_url, cfg.mqtt_instance_url);
        assert_eq!(back.mqtt_client_id, cfg.mqtt_client_id);
        assert_eq!(back.mqtt_username, cfg.mqtt_username);
        assert_eq!(back.mqtt_password, cfg.mqtt_password);
        assert_eq!(back.mqtt_namespace, cfg.mqtt_namespace);
    }

    /// `Display` is used in operator-facing logs. It must never render the
    /// password, which sits in the same struct one field away.
    #[test]
    fn display_does_not_leak_the_password() {
        let cfg = MqttConfig {
            mqtt_password: Some("hunter2".to_string()),
            ..MqttConfig::default()
        };
        let shown = cfg.to_string();
        assert!(!shown.contains("hunter2"), "Display leaked the password: {shown}");
        assert!(shown.contains("mqtt://localhost:1883"));
    }
}
