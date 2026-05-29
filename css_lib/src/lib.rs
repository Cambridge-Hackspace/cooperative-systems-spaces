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

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

pub mod wire;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
