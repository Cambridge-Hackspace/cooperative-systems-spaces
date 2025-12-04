use anyhow::Result;
use clap::Parser;
use tracing::info;

mod config;
use config::{generate_sample_config, load_config, ConfigManager};
use crate::config::AuthStatus;

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

    // Create configuration manager for runtime reloading
    let config_manager = ConfigManager::new(
        app_config.clone(),
        Some(std::path::PathBuf::from(&args.config)),
    );

    // Display MQTT status
    if let Some(mqtt) = &app_config.local_mqtt_config {
        info!("Local MQTT enabled - connecting to: {}", mqtt.mqtt_instance_url);
    } else {
        info!("Local MQTT disabled");
    }

    match app_config.auth_status {
        AuthStatus::Unauthenticated => {
            info!("Edge client is unauthenticated, please use cli --auth subcommand or web UI to authenticate");
        }
        AuthStatus::Pending => {
            info!("Edge client authentication is pending on server, please wait");
        }
        AuthStatus::Approved => {
            info!("Edge client is authenticated");
            // Display MQTT status
            if let Some(mqtt) = &app_config.remote_mqtt_config {
                info!("Remote MQTT enabled - connecting to: {}", mqtt.mqtt_instance_url);
            } else {
                info!("Remote MQTT disabled");
            }
        }
        AuthStatus::Denied => {
            info!("Edge client authentication request was denied on server");
        }
    }
    // TODO: Add your implementation here
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
