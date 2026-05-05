use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::path::Path;
use uuid::Uuid;
use crate::config::{Config, AuthStatus, DeviceInfo, MqttConfig};
use crate::system_info::get_system_info;

#[derive(Debug, Serialize)]
struct RegisterDeviceRequest {
    device_code: String,
    name: String,
    kind: String,
    mac_address: String,
    software_version: String,
    ipv4_address: Option<String>,
    ipv6_address: Option<String>,
    platform: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegisterDeviceResponse {
    device_id: Uuid,
    auth_token: String,
    device_name: String,
    mqtt_config: Option<EdgeMqttConfig>,
}

#[derive(Debug, Deserialize)]
struct EdgeMqttConfig {
    mqtt_instance_url: String,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
    mqtt_namespace: String,
}

/// Register the edge apparatus with the Space Server
pub async fn register_device(
    instance_url: &str,
    device_code: &str,
    config: &Config,
    config_path: &Path,
) -> Result<()> {
    info!("Starting device registration with instance: {}", instance_url);
    
    // Collect system information
    let system_info = get_system_info()?;
    
    // Prepare registration request
    let register_url = format!("{}/api/devices/register", instance_url.trim_end_matches('/'));
    let request = RegisterDeviceRequest {
        device_code: device_code.to_string(),
        name: config.name.clone(),
        kind: "Edge".to_string(),
        mac_address: system_info.mac_address,
        software_version: env!("CARGO_PKG_VERSION").to_string(),
        ipv4_address: system_info.ipv4_address,
        ipv6_address: system_info.ipv6_address,
        platform: system_info.platform,
    };
    
    info!("Sending registration request...");
    
    // Send registration request
    let client = Client::new();
    let response = client
        .post(&register_url)
        .json(&request)
        .send()
        .await
        .context("Failed to send registration request")?;
    
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        error!("Registration failed with status {}: {}", status, error_text);
        return Err(anyhow::anyhow!("Registration failed: {} - {}", status, error_text));
    }
    
    // Parse the ApiResponse wrapper
    let api_response: ApiResponse<RegisterDeviceResponse> = response
        .json()
        .await
        .context("Failed to parse registration response")?;
    
    // Check if the API call was successful
    if !api_response.success {
        let error_msg = api_response.error.unwrap_or_else(|| "Unknown error".to_string());
        error!("Registration failed: {}", error_msg);
        return Err(anyhow::anyhow!("Registration failed: {}", error_msg));
    }
    
    let registration_response = api_response.data
        .ok_or_else(|| anyhow::anyhow!("No data in registration response"))?;
    
    info!("Registration successful! Device ID: {}", registration_response.device_id);
    
    // Update configuration with device credentials
    let mut new_config = config.clone();
    new_config.name = registration_response.device_name.clone();
    new_config.auth_status = AuthStatus::Approved;
    new_config.remote_device_info = Some(DeviceInfo {
        remote_id: registration_response.device_id.to_string(),
        remote_auth_token: registration_response.auth_token.clone(),
        remote_instance_url: instance_url.trim_end_matches('/').to_string(),
    });
    
    // Configure MQTT if provided by server
    if let Some(mqtt) = registration_response.mqtt_config {
        info!("Configuring MQTT connection...");
        info!("  MQTT URL: {}", mqtt.mqtt_instance_url);
        info!("  MQTT Namespace: {}", mqtt.mqtt_namespace);
        
        new_config.remote_mqtt_config = Some(MqttConfig {
            mqtt_instance_url: mqtt.mqtt_instance_url,
            mqtt_client_id: registration_response.device_id.to_string(),
            mqtt_username: mqtt.mqtt_username,
            mqtt_password: mqtt.mqtt_password,
            mqtt_namespace: mqtt.mqtt_namespace,
        });
        
        info!("MQTT configuration applied successfully");
    } else {
        info!("No MQTT configuration provided by server");
    }
    
    // Save updated configuration
    new_config.to_file(config_path)
        .context("Failed to save updated configuration")?;
    
    info!("Configuration updated and saved successfully");
    info!("Device '{}' is now registered and ready to use", new_config.name);
    
    Ok(())
}

/// Check if device is already registered
pub fn is_registered(config: &Config) -> bool {
    matches!(config.auth_status, AuthStatus::Approved) && config.remote_device_info.is_some()
}

/// Get device ID if registered
pub fn get_device_id(config: &Config) -> Option<String> {
    config.remote_device_info.as_ref().map(|info| info.remote_id.clone())
}

/// Get auth token if registered
pub fn get_auth_token(config: &Config) -> Option<String> {
    config.remote_device_info.as_ref().map(|info| info.remote_auth_token.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_registered() {
        let mut config = Config::default();
        assert!(!is_registered(&config));
        
        config.auth_status = AuthStatus::Approved;
        config.remote_device_info = Some(DeviceInfo {
            remote_id: "test-device".to_string(),
            remote_auth_token: "test-token".to_string(),
            remote_instance_url: "https://spaces.neiam.org".to_string(),
        });
        
        assert!(is_registered(&config));
    }

    #[test]
    fn test_get_device_id() {
        let mut config = Config::default();
        assert_eq!(get_device_id(&config), None);
        
        config.remote_device_info = Some(DeviceInfo {
            remote_id: "test-device".to_string(),
            remote_auth_token: "test-token".to_string(),
            remote_instance_url: "https://spaces.neiam.org".to_string(),
        });
        
        assert_eq!(get_device_id(&config), Some("test-device".to_string()));
    }
}
