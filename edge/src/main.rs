use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{error, info, warn};
use std::sync::{Arc, RwLock};

mod config;
mod registration;
mod system_info;
mod mqtt;
mod web_server;

use config::{generate_sample_config, load_config};
use crate::config::AuthStatus;
use crate::registration::{register_device, is_registered};
use crate::mqtt::{EdgeMqttClient, run_mqtt_event_loop};
use crate::web_server::start_web_server;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Enable verbose output (-v for INFO, -vv for DEBUG, -vvv for TRACE)
    #[arg(long, short = 'v', action = clap::ArgAction::Count)]
    verbose: u8,

    /// Path to the configuration file
    #[arg(short, long, default_value = "./edge.config.toml")]
    config: String,

    /// Generate a sample configuration file and exit
    #[arg(long)]
    generate_config: bool,
    
    #[command(subcommand)]
    command: Option<Commands>,

    /// FRONTEND_PATH environment variable
    #[arg(long, env = "FRONTEND_PATH", default_value = "./frontend_edge/dist")]
    frontend_path: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Register this device with a Space Server
    Register {
        /// Space Server instance URL (e.g., https://space.example.com)
        #[arg(short, long)]
        instance_url: String,
        
        /// Device registration code (8 emojis)
        #[arg(short = 'c', long)]
        code: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_logging(args.verbose);

    info!("Edge binary started");

    // Handle config generation
    if args.generate_config {
        generate_sample_config(&args.config)?;
        return Ok(());
    }

    // Load configuration
    let app_config = load_config(&args.config)?;
    info!("Configuration loaded from: {}", args.config);
    info!("Edge client name: {}", app_config.name);

    // Handle subcommands
    if let Some(command) = args.command {
        match command {
            Commands::Register { instance_url, code } => {
                if is_registered(&app_config) {
                    info!("Device is already registered!");
                    info!("If you want to re-register, please update the config file to reset auth_status.");
                    return Ok(());
                }
                
                info!("Registering device with Space Server...");
                register_device(
                    &instance_url,
                    &code,
                    &app_config,
                    std::path::Path::new(&args.config),
                )
                .await?;
                
                info!("Registration complete! Please restart the edge device.");
                return Ok(());
            }
        }
    }

    // Store config path for name updates
    std::env::set_var("CONFIG_PATH", &args.config);

    // Create configuration manager for runtime reloading
    let config_arc = Arc::new(RwLock::new(app_config.clone()));
    
    // Get web server port from config or use default
    let web_port = 8080; // You can add this to config if needed

    // Display MQTT status
    if let Some(mqtt) = &app_config.local_mqtt_config {
        info!("Local MQTT enabled - connecting to: {}", mqtt.mqtt_instance_url);
    } else {
        info!("Local MQTT disabled");
    }

    match app_config.auth_status {
        AuthStatus::Unauthenticated => {
            info!("Edge client is unauthenticated");
            info!("Web UI available at http://localhost:{} for registration", web_port);
            info!("Or use: edge register --instance-url <url> --code <code>");
            
            // Start web server for registration
            start_web_server(config_arc, args.config, web_port).await?;
        }
        AuthStatus::Pending => {
            info!("Edge client authentication is pending on server, please wait");
            info!("Web UI available at http://localhost:{} for status", web_port);
            
            // Start web server to show status
            start_web_server(config_arc, args.config, web_port).await?;
        }
        AuthStatus::Approved => {
            info!("Edge client is authenticated");
            info!("Web UI available at http://localhost:{} for status", web_port);
            
            // Check if remote MQTT is configured
            if app_config.remote_mqtt_config.is_none() {
                info!("Remote MQTT not configured - running without MQTT connection");
                info!("Edge device running without MQTT. Press Ctrl+C to exit.");
                
                // Start web server to show status
                start_web_server(config_arc, args.config, web_port).await?;
                return Ok(());
            }
            
            info!("Starting MQTT connection...");
            
            // Create MQTT client
            let (mqtt_client, rx) = EdgeMqttClient::new(&app_config, config_arc.clone()).await?;
            let mqtt_client = Arc::new(mqtt_client);
            
            // Subscribe to command topics
            mqtt_client.subscribe_to_commands()?;
            
            // Publish initial heartbeat
            mqtt_client.publish_heartbeat()?;
            
            // Publish initial device data
            mqtt_client.publish_device_data()?;
            
            // Start background tasks
            mqtt_client.start_heartbeat_task();
            mqtt_client.start_data_publisher_task();
            
            info!("Edge device running. Press Ctrl+C to exit.");
            
            // Start web server in background
            let config_for_web = config_arc.clone();
            let config_path_for_web = args.config.clone();
            tokio::spawn(async move {
                if let Err(e) = start_web_server(config_for_web, config_path_for_web, web_port).await {
                    error!("Web server error: {}", e);
                }
            });
            
            // Run MQTT event loop with graceful shutdown
            tokio::select! {
                result = run_mqtt_event_loop(rx, mqtt_client.clone()) => {
                    if let Err(e) = result {
                        error!("MQTT event loop exited with error: {}", e);
                    } else {
                        info!("MQTT event loop exited normally");
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received shutdown signal (Ctrl+C)");
                }
            }
            
            // Graceful shutdown
            info!("Shutting down gracefully...");
            if let Err(e) = mqtt_client.disconnect() {
                warn!("Error disconnecting from MQTT broker: {}", e);
            }
        }
        AuthStatus::Denied => {
            info!("Edge client authentication request was denied on server");
            info!("Please contact your administrator");
            return Ok(());
        }
    }
    
    info!("Shutting down...");
    Ok(())
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => tracing::Level::INFO,
        1 => tracing::Level::INFO,
        2 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();
}
