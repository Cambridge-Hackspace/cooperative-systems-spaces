use anyhow::Result;
use clap::{Parser, Subcommand};
use reqwest::Client;
use std::sync::{Arc, RwLock};
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

mod calendar;
mod config;
mod registration;
mod system_info;
mod mqtt;
mod toolguard;
mod doors;
mod edge_inbound;
mod ws;
mod web_server;

use config::{generate_sample_config, load_config};
use crate::config::AuthStatus;
use crate::registration::{register_device, is_registered};
use crate::mqtt::{EdgeMqttClient, LocalMqttClient, run_mqtt_event_loop, run_local_mqtt_event_loop};
use crate::toolguard::ToolGuardState;
use crate::doors::DoorsState;
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

    init_logging(args.verbose);

    info!("Edge binary started");

    if args.generate_config {
        generate_sample_config(&args.config)?;
        return Ok(());
    }

    let app_config = load_config(&args.config)?;
    info!("Configuration loaded from: {}", args.config);
    info!("Edge client name: {}", app_config.name);

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

                info!("Registration complete! Please restart the edge apparatus.");
                return Ok(());
            }
        }
    }

    std::env::set_var("CONFIG_PATH", &args.config);

    let config_arc = Arc::new(RwLock::new(app_config.clone()));
    let web_port = 8080;

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
            start_web_server(config_arc, args.config, web_port, Arc::new(ToolGuardState::new()), args.frontend_path).await?;
        }
        AuthStatus::Pending => {
            info!("Edge client authentication is pending on server, please wait");
            info!("Web UI available at http://localhost:{} for status", web_port);
            start_web_server(config_arc, args.config, web_port, Arc::new(ToolGuardState::new()), args.frontend_path).await?;
        }
        AuthStatus::Approved => {
            info!("Edge client is authenticated");
            info!("Web UI available at http://localhost:{} for status", web_port);

            // Shared toolguard state — notify_rx fires on every state change
            let (toolguard_state_inner, state_notify_rx) = ToolGuardState::new_with_notify();
            let toolguard_state = Arc::new(toolguard_state_inner);

            // Shared door cache + cross-client bridges. `doors_unlock_*` flows
            // remote → local (server-issued unlocks → local relay).
            // `doors_event_*` flows local → remote (scans → server audit log).
            let doors_state = DoorsState::new();
            let (doors_unlock_tx, doors_unlock_rx) =
                tokio::sync::mpsc::unbounded_channel::<crate::doors::UnlockCommand>();
            let (doors_event_tx, doors_event_rx) =
                tokio::sync::mpsc::unbounded_channel::<crate::doors::DoorsEvent>();

            // Extract device credentials once
            let device_info = app_config.remote_device_info.clone()
                .expect("Approved device must have remote_device_info");
            let remote_instance_url = device_info.remote_instance_url.clone();
            let remote_auth_token = device_info.remote_auth_token.clone();
            let sync_interval_secs = app_config.toolguard_sync_interval_secs;

            // ── Boot-reset: tell server to clear any InUse tools ────────────
            {
                let url = format!("{}/api/toolguard/boot-reset", remote_instance_url);
                match Client::new()
                    .post(&url)
                    .bearer_auth(&remote_auth_token)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        info!("Boot-reset: server acknowledged ({} {})", resp.status().as_u16(), url);
                    }
                    Ok(resp) => {
                        warn!("Boot-reset returned HTTP {}", resp.status());
                    }
                    Err(e) => {
                        warn!("Boot-reset request failed: {}", e);
                    }
                }
            }

            // ── Periodic HTTP sync task ──────────────────────────────────────
            {
                let state = Arc::clone(&toolguard_state);
                let instance_url = remote_instance_url.clone();
                let auth_token = remote_auth_token.clone();
                tokio::spawn(async move {
                    let client = Client::new();
                    let mut ticker = interval(Duration::from_secs(sync_interval_secs));
                    loop {
                        ticker.tick().await;
                        let url = format!("{}/api/toolguard/sync", instance_url);
                        match client
                            .get(&url)
                            .bearer_auth(&auth_token)
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                match resp.bytes().await {
                                    Ok(bytes) => match state.apply_sync_bytes(&bytes) {
                                        Ok(()) => info!("ToolGuard state synced from remote"),
                                        Err(e) => warn!("Failed to parse toolguard sync response: {}", e),
                                    },
                                    Err(e) => warn!("Failed to read toolguard sync body: {}", e),
                                }
                            }
                            Ok(resp) => warn!("ToolGuard sync returned HTTP {}", resp.status()),
                            Err(e) => warn!("ToolGuard sync request failed: {}", e),
                        }
                    }
                });
            }

            // ── Local MQTT client (if configured) ────────────────────────────
            // state_notify_rx is consumed by the local event loop; if no local MQTT
            // is configured we drop it so the sender's try_send just silently discards.
            let state_notify_rx = Some(state_notify_rx);
            // The local client owns the consumer side of the unlock bridge so
            // it can fire the relay when the server pushes a `doors/unlock`.
            // Wrapped in an Option so we can `.take()` it into the spawned task.
            let mut doors_unlock_rx = Some(doors_unlock_rx);
            if let Some(local_mqtt_cfg) = &app_config.local_mqtt_config {
                match LocalMqttClient::new(
                    local_mqtt_cfg,
                    Arc::clone(&toolguard_state),
                    remote_instance_url.clone(),
                    remote_auth_token.clone(),
                    Arc::clone(&doors_state),
                    doors_event_tx.clone(),
                ).await {
                    Ok((local_client, local_rx)) => {
                        let local_client = Arc::new(local_client);
                        if let Err(e) = local_client.subscribe_to_requests() {
                            error!("Failed to subscribe to local toolguard topics: {}", e);
                        } else {
                            let lc = Arc::clone(&local_client);
                            let rx = state_notify_rx.expect("state_notify_rx consumed once");
                            tokio::spawn(async move {
                                if let Err(e) = run_local_mqtt_event_loop(local_rx, lc, rx).await {
                                    error!("Local MQTT event loop error: {}", e);
                                }
                            });

                            local_client.start_calendar_publisher_task(
                                app_config.calendar_mqtt_topic.clone(),
                                app_config.calendar_sync_interval_secs,
                            );

                            // Bridge: server-initiated unlocks → local relay.
                            if let Some(mut rx) = doors_unlock_rx.take() {
                                let lc = Arc::clone(&local_client);
                                tokio::spawn(async move {
                                    while let Some(cmd) = rx.recv().await {
                                        lc.publish_doors_unlock_response(&cmd);
                                    }
                                });
                            }
                        }
                    }
                    Err(e) => error!("Failed to start local MQTT client: {}", e),
                }
            }
            // If no local client was started, drop the receiver explicitly so
            // remote senders see a closed channel and don't pile up forever.
            drop(doors_unlock_rx);

            // ── Shared inbound dispatcher (used by whichever remote transport runs) ──
            let edge_inbound = std::sync::Arc::new(crate::edge_inbound::EdgeInbound {
                config_manager: config_arc.clone(),
                toolguard_state: Arc::clone(&toolguard_state),
                doors_state: Arc::clone(&doors_state),
                doors_unlock_tx: doors_unlock_tx.clone(),
            });

            // ── Web server (independent of transport) ────────────────────────
            let config_for_web = config_arc.clone();
            let config_path_for_web = args.config.clone();
            let frontend_path_for_web = args.frontend_path.clone();
            let tgs_for_web = Arc::clone(&toolguard_state);
            tokio::spawn(async move {
                if let Err(e) = start_web_server(config_for_web, config_path_for_web, web_port, tgs_for_web, frontend_path_for_web).await {
                    error!("Web server error: {}", e);
                }
            });

            // ── Remote transport: MQTT or WebSocket ──────────────────────────
            use crate::config::RemoteTransport;
            match app_config.remote_transport {
                RemoteTransport::Mqtt => {
                    if app_config.remote_mqtt_config.is_none() {
                        info!("Remote MQTT not configured - running without remote connection");
                        // Drop the doors-event receiver so local scans don't queue indefinitely.
                        drop(doors_event_rx);
                        tokio::signal::ctrl_c().await.ok();
                        return Ok(());
                    }

                    info!("Starting remote MQTT connection...");
                    let (mqtt_client, rx) = EdgeMqttClient::new(
                        &app_config,
                        edge_inbound.clone(),
                    ).await?;
                    let mqtt_client = Arc::new(mqtt_client);

                    mqtt_client.subscribe_to_commands()?;
                    mqtt_client.publish_heartbeat()?;
                    mqtt_client.publish_device_data()?;
                    mqtt_client.start_heartbeat_task();
                    mqtt_client.start_data_publisher_task();

                    // Bridge: local RFID scans → server `doors/event`.
                    {
                        let mc = Arc::clone(&mqtt_client);
                        let mut rx = doors_event_rx;
                        tokio::spawn(async move {
                            while let Some(event) = rx.recv().await {
                                if let Err(e) = mc.publish_doors_event(&event) {
                                    warn!("Failed to publish doors/event upstream: {}", e);
                                }
                            }
                        });
                    }

                    info!("edge apparatus running on MQTT transport. Press Ctrl+C to exit.");
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
                    info!("Shutting down gracefully...");
                    if let Err(e) = mqtt_client.disconnect() {
                        warn!("Error disconnecting from remote MQTT broker: {}", e);
                    }
                }
                RemoteTransport::Websocket => {
                    info!("Starting remote WebSocket connection...");
                    let ws_client = crate::ws::WsClient::start(
                        &remote_instance_url,
                        remote_auth_token.clone(),
                        edge_inbound.clone(),
                    )?;

                    // Bridge: local RFID scans → server `doors/event`.
                    {
                        let wc = ws_client.clone();
                        let mut rx = doors_event_rx;
                        tokio::spawn(async move {
                            while let Some(event) = rx.recv().await {
                                if let Err(e) = wc.publish_doors_event(&event) {
                                    warn!("Failed to publish doors/event upstream: {}", e);
                                }
                            }
                        });
                    }

                    info!("edge apparatus running on WebSocket transport. Press Ctrl+C to exit.");
                    tokio::signal::ctrl_c().await.ok();
                    info!("Shutting down gracefully...");
                }
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
