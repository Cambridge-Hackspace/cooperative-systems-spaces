use anyhow::{Context, Result};
use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::registration::{get_auth_token, get_device_id};
use crate::system_info::get_system_info;

#[derive(Debug, Serialize)]
struct DeviceData {
    mac_address: String,
    software_version: String,
    ipv4_address: Option<String>,
    ipv6_address: Option<String>,
    uptime: u64,
    platform: String,
}

#[derive(Debug, Deserialize)]
struct NameUpdateMessage {
    name: String,
}

/// MQTT Client Manager for edge device
pub struct EdgeMqttClient {
    client: mqtt::AsyncClient,
    device_id: String,
    namespace: String,
    start_time: std::time::Instant,
    config_manager: std::sync::Arc<std::sync::RwLock<Config>>,
}

impl EdgeMqttClient {
    /// Create a new MQTT client
    pub async fn new(
        config: &Config,
        config_manager: std::sync::Arc<std::sync::RwLock<Config>>,
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
        
        let client = mqtt::AsyncClient::new(create_opts)
            .context("Failed to create MQTT client")?;
        
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
            conn_opts_builder.user_name(&device_id).password(&auth_token);
        }
        
        let conn_opts = conn_opts_builder.finalize();
        
        // Connect to the broker
        client.connect(conn_opts).wait()
            .context("Failed to connect to MQTT broker")?;
        
        info!("Connected to MQTT broker at {} with namespace: {}", 
              mqtt_config.mqtt_instance_url, mqtt_config.mqtt_namespace);
        
        Ok((
            Self {
                client,
                device_id,
                namespace: mqtt_config.mqtt_namespace.clone(),
                start_time: std::time::Instant::now(),
                config_manager,
            },
            rx,
        ))
    }
    
    /// Subscribe to device command topics
    pub fn subscribe_to_commands(&self) -> Result<()> {
        let name_topic = format!("{}/devices/{}/name", self.namespace, self.device_id);
        
        self.client
            .subscribe(&name_topic, 1)
            .wait()
            .context("Failed to subscribe to name update topic")?;
        
        info!("Subscribed to command topics with namespace: {}", self.namespace);
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
        
        let payload = serde_json::to_string(&data)
            .context("Failed to serialize device data")?;
        
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
    
    /// Handle incoming MQTT messages
    pub async fn handle_message(&self, topic: &str, payload: &[u8]) -> Result<()> {
        let name_topic = format!("{}/devices/{}/name", self.namespace, self.device_id);
        
        if topic == name_topic {
            // Handle name update
            match serde_json::from_slice::<NameUpdateMessage>(payload) {
                Ok(msg) => {
                    info!("Received name update: {}", msg.name);
                    self.update_device_name(&msg.name).await?;
                }
                Err(e) => {
                    warn!("Failed to parse name update message: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Update device name in config
    async fn update_device_name(&self, new_name: &str) -> Result<()> {
        info!("Updating device name to: {}", new_name);
        
        // Update config
        {
            let mut config = self.config_manager.write().unwrap();
            config.name = new_name.to_string();
            
            // Save to file
            if let Some(path) = std::env::var_os("CONFIG_PATH") {
                config.to_file(path)?;
            }
        }
        
        info!("Device name updated successfully");
        info!("Note: Restart the device for the name change to take full effect");
        
        Ok(())
    }
    
    /// Disconnect from MQTT broker gracefully
    pub fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from MQTT broker...");
        self.client.disconnect(None).wait()
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

#[cfg(test)]
mod tests {
    use super::*;

    // Tests can be added here for MQTT functionality
}
