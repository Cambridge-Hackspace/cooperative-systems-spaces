use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

/// MQTT Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    /// MQTT broker URL used by the CSS Instance
    pub mqtt_instance_url: String,

    /// MQTT broker URL used by the CSS Edge Client
    pub mqtt_edge_url: String,

    /// MQTT Client ID used by the CSS Edge Client
    pub mqtt_client_id: String,

    /// MQTT username used by the CSS Edge Client
    pub mqtt_username: Option<String>,

    /// MQTT password used by the CSS Edge Client
    pub mqtt_password: Option<String>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            mqtt_instance_url: "mqtt://localhost:1883".to_string(),
            mqtt_edge_url: "mqtt://localhost:1883".to_string(),
            mqtt_client_id: "css-edge-001".to_string(),
            mqtt_username: None,
            mqtt_password: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthStatus {
    Unauthenticated,
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct  DeviceInfo {
    remote_id: String,
    remote_auth_token: String,
}

/// Main edge configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Edge client display name (leave empty for auto-generation)
    #[serde(default)]
    pub name: String,

    /// Authentication Status
    pub auth_status: AuthStatus,
    
    /// MQTT configuration local
    #[serde(default)]
    pub local_mqtt_config: Option<MqttConfig>,

    /// MQTT configuration remote
    #[serde(default)]
    pub remote_mqtt_config: Option<MqttConfig>,

    /// Remote Device Info
    pub remote_device_info: Option<DeviceInfo>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: generate_name(),
            auth_status: AuthStatus::Unauthenticated,
            local_mqtt_config: None,
            remote_mqtt_config: None,
            remote_device_info: None,
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        let config: Config = toml::from_str(&content)
            .with_context(|| "Failed to parse TOML configuration")?;
        
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize configuration to TOML")?;
        
        // Create parent directories if they don't exist
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }
        
        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file: {}", path.as_ref().display()))?;
        
        Ok(())
    }
}

/// Configuration manager for runtime reloading
pub struct ConfigManager {
    config: Arc<RwLock<Config>>,
    config_path: Option<std::path::PathBuf>,
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new(config: Config, config_path: Option<std::path::PathBuf>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        }
    }

    /// Get the current configuration (read-only)
    pub fn get_config(&self) -> Config {
        self.config.read().unwrap().clone()
    }

    /// Reload configuration from disk
    pub fn reload_config(&self) -> Result<()> {
        let config_path = self.config_path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No config path available for reloading"))?;

        info!("Reloading configuration from: {}", config_path.display());

        let new_config = Config::from_file(config_path)
            .with_context(|| "Failed to reload configuration")?;

        // Validate the new configuration
        self.validate_config(&new_config)?;

        // Update the configuration atomically
        {
            let mut config_guard = self.config.write().unwrap();
            *config_guard = new_config;
        }

        info!("Configuration reloaded successfully");
        Ok(())
    }

    /// Validate configuration before applying
    fn validate_config(&self, config: &Config) -> Result<()> {
        // Validate MQTT configuration if present
        if let Some(mqtt) = &config.local_mqtt_config {
            if mqtt.mqtt_password == Some("change-me-in-production".to_string()) {
                warn!("MQTT password is still set to default value - this is insecure for production");
            }

            if mqtt.mqtt_client_id.is_empty() {
                return Err(anyhow::anyhow!("MQTT client ID cannot be empty"));
            }

            if mqtt.mqtt_instance_url.is_empty() {
                return Err(anyhow::anyhow!("MQTT instance URL cannot be empty"));
            }

            if mqtt.mqtt_edge_url.is_empty() {
                return Err(anyhow::anyhow!("MQTT edge URL cannot be empty"));
            }
        }

        // Validate MQTT configuration if present
        if let Some(mqtt) = &config.remote_mqtt_config {
            if mqtt.mqtt_password == Some("change-me-in-production".to_string()) {
                warn!("MQTT password is still set to default value - this is insecure for production");
            }

            if mqtt.mqtt_client_id.is_empty() {
                return Err(anyhow::anyhow!("MQTT client ID cannot be empty"));
            }

            if mqtt.mqtt_instance_url.is_empty() {
                return Err(anyhow::anyhow!("MQTT instance URL cannot be empty"));
            }

            if mqtt.mqtt_edge_url.is_empty() {
                return Err(anyhow::anyhow!("MQTT edge URL cannot be empty"));
            }
        }

        Ok(())
    }

    /// Get a thread-safe reference to the configuration
    pub fn get_config_ref(&self) -> Arc<RwLock<Config>> {
        Arc::clone(&self.config)
    }
}

/// Load configuration from file or create default configuration
pub fn load_config<P: AsRef<Path>>(config_path: P) -> Result<Config> {
    let path = config_path.as_ref();
    
    if path.exists() {
        info!("Loading configuration from: {}", path.display());
        Config::from_file(path)
    } else {
        info!("Config file not found. Creating default configuration at: {}", path.display());
        let default_config = Config::default();
        
        // Save default configuration to file
        default_config.to_file(path)
            .with_context(|| "Failed to create default configuration file")?;
        
        info!("Default configuration file created. Please review and modify as needed.");
        Ok(default_config)
    }
}

/// Generate a sample configuration file with comments
pub fn generate_sample_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let default_config = Config::default();
    
    default_config.to_file(&path)
        .with_context(|| "Failed to write sample configuration file")?;
    
    info!("Sample configuration file generated at: {}", path.as_ref().display());
    info!("Please review and modify the configuration as needed.");
    Ok(())
}

/// Generate a random name in the format: adjective-color-animal
fn generate_name() -> String {
    use rand::prelude::SliceRandom;

    let adjectives = [
        "quaint", "clever", "brave", "calm", "bright", "swift", "gentle", "noble", "quiet", "wise",
        "happy", "proud", "kind", "bold", "fair", "keen", "deft", "crisp", "alert", "agile",
        "vivid", "zesty", "witty", "sunny", "eager", "fierce", "graceful", "honest", "jolly",
        "lively", "mellow", "nimble",
    ];

    let colors = [
        "red", "blue", "green", "yellow", "purple", "orange", "pink", "teal", "amber", "coral",
        "crimson", "azure", "olive", "jade", "ruby", "gold", "silver", "bronze", "cyan", "magenta",
        "violet", "indigo", "turquoise", "scarlet", "emerald", "sapphire", "pearl", "ivory",
        "ebony", "cobalt", "lavender", "maroon",
    ];

    let animals = [
        "turtle", "falcon", "dolphin", "tiger", "eagle", "wolf", "bear", "fox", "hawk", "lion",
        "panda", "otter", "raven", "badger", "lynx", "moose", "owl", "seal", "deer", "crane",
        "swan", "penguin", "raccoon", "ferret", "gazelle", "cheetah", "jaguar", "panther",
        "leopard", "bison", "buffalo", "cobra",
    ];

    let mut rng = rand::thread_rng();

    let adjective = adjectives.choose(&mut rng).unwrap();
    let color = colors.choose(&mut rng).unwrap();
    let animal = animals.choose(&mut rng).unwrap();

    format!("{}-{}-{}", adjective, color, animal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        
        // Verify we can deserialize it back
        let _: Config = toml::from_str(&toml_str).unwrap();
    }

    #[test]
    fn test_config_file_roundtrip() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = Config::default();
        
        // Save to file
        config.to_file(temp_file.path()).unwrap();
        
        // Load from file
        let loaded_config = Config::from_file(temp_file.path()).unwrap();
        
        // Compare (using debug format since we don't implement PartialEq)
        assert_eq!(format!("{:?}", config), format!("{:?}", loaded_config));
    }

    #[test]
    fn test_load_config_creates_default_if_missing() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();
        
        // Delete the file so it doesn't exist
        std::fs::remove_file(path).unwrap();
        
        // load_config should create a default config
        let _config = load_config(path).unwrap();
        
        // File should now exist
        assert!(path.exists());
    }

    #[test]
    fn test_generate_name_format() {
        let name = generate_name();
        let parts: Vec<&str> = name.split('-').collect();
        
        // Should have three parts
        assert_eq!(parts.len(), 3);
        
        // Each part should not be empty
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
        assert!(!parts[2].is_empty());
    }
}
