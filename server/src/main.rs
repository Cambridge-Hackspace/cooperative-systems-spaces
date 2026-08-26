use axum::routing::get;
use axum::Router;
use clap::Parser;
use css_server::devices_inbound::DeviceInbound;
use css_server::{api, config, root, AppState};
use dr_metrix_axum::{metrics_handler, PrometheusMetrics};
use dr_metrix_core::collector::CollectorConfig;
use dr_metrix_postgres::PostgresMetrics;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use css_server::calendar::CalendarService;
use css_server::config::{load_config, ConfigManager};
use css_server::database::{initialize_database, DatabaseManager};
use css_server::devices_transport::{DeviceChannelRegistry, DeviceTransport};
use css_server::doors::DoorService;
use css_server::mfa::MfaService;
use css_server::mqtt::MqttService;
use css_server::pages::PagesService;
use css_server::profile::{AuditLogger, ProfileValidator};
use css_server::recaptcha::RecaptchaService;
use css_server::throttle::RegistrationThrottleService;
use css_server::webhooks::WebhookDispatcher;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(long, env = "CONFIG_PATH", default_value = "config.toml")]
    config_path: String,

    /// Generate a sample configuration file and exit
    #[arg(long)]
    generate_config: bool,

    /// FRONTEND_PATH environment variable
    #[arg(long, env = "FRONTEND_PATH", default_value = "./frontend/dist")]
    frontend_path: String,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Booting CS-Spaces");

    // Parse command line arguments
    let args = Args::parse();

    // Handle config generation
    if args.generate_config {
        config::generate_sample_config(&args.config_path)?;
        return Ok(());
    }

    // Load configuration
    // A configuration that had to be rewritten is not a crash and it is not a
    // success. It gets its own exit code -- 78, sysexits.h's EX_CONFIG -- so a
    // supervisor can tell "this needs a human to look at the config" apart from
    // both "the server stopped normally" and "the server fell over". The loader
    // used to exit(0) here, which told every one of them the opposite.
    let app_config = match load_config(&args.config_path) {
        Ok(config) => config,
        Err(e) => {
            if let Some(rewritten) = e.downcast_ref::<css_server::config::ConfigRewritten>() {
                eprintln!("\n{rewritten}\n");
                std::process::exit(78);
            }
            return Err(e);
        }
    };
    info!("Configuration loaded from: {}", args.config_path);
    info!(
        "Site: {} running in {} mode",
        app_config.site.site_name,
        if app_config.site.debug {
            "debug"
        } else {
            "production"
        }
    );

    // Create configuration manager
    let config_manager = Arc::new(ConfigManager::new(
        app_config.clone(),
        Some(std::path::PathBuf::from(&args.config_path)),
    ));

    // Initialize database
    info!("Initializing database connection pool...");
    let db_manager = Arc::new(initialize_database(&app_config.database).await?);
    info!("Database connection pool initialized successfully");

    // Test basic database operations
    info!("Testing basic database operations...");
    test_database_operations(&db_manager).await?;
    info!("Database operations test completed successfully");

    // Bootstrap the versioned profile-field schema: the DB is authoritative
    // once any version exists; on a fresh install, seed version 1 from the
    // config file so existing deployments carry their fields forward.
    match db_manager.get_latest_profile_config_version()? {
        Some(latest) => {
            let fields: Vec<config::ProfileField> = serde_json::from_value(latest.profile_fields)?;
            config_manager.set_profile_fields(fields);
            info!(
                "Loaded profile field schema from database (version {})",
                latest.version
            );
        }
        None => {
            let seed = serde_json::to_value(&app_config.user.profile_fields)?;
            db_manager.insert_profile_config_version(seed, None)?;
            info!("Seeded profile field schema version 1 from config file");
        }
    }

    // Setup Prometheus metrics
    let prom = Arc::new(
        PrometheusMetrics::builder("css")
            .with_process_collector()
            .build()?,
    );
    let pg_config = CollectorConfig {
        namespace: "css".into(),
        ..Default::default()
    };
    let pg_metrics = PostgresMetrics::new(db_manager.pool().clone(), pg_config.clone())?;
    prom.add_collector(pg_metrics, pg_config.collect_interval)?;

    let profile_validator = ProfileValidator::new(&app_config.user);
    let audit_logger = AuditLogger::new(db_manager.clone());
    let throttle_service = Arc::new(RegistrationThrottleService::new());
    let recaptcha_service = Arc::new(RecaptchaService::new(
        app_config
            .registration_challenge
            .recaptcha_secret_key
            .clone(),
    ));

    // Initialize calendar service
    info!("Initializing calendar service...");
    let calendar_service = Arc::new(tokio::sync::RwLock::new(CalendarService::new(
        app_config.calendar.clone(),
    )));
    info!("Calendar service initialized");

    // Initialize pages service
    info!("Initializing pages service...");
    let pages_service = Arc::new(tokio::sync::RwLock::new(
        PagesService::new(app_config.pages.clone()).await?,
    ));
    info!("Pages service initialized");

    // Shared transport-agnostic inbound dispatcher and per-device session
    // registry. Created before MQTT so MqttService can take a reference.
    let device_inbound = Arc::new(DeviceInbound::new(
        db_manager.clone(),
        app_config.toolguard.profile_field.clone(),
    ));
    let device_registry = DeviceChannelRegistry::new();

    // Initialize MQTT service if edge is enabled
    let mqtt_service_arc = if app_config.edge.edge_enabled {
        if let Some(mqtt_config) = &app_config.edge.edge_mqtt_config {
            info!("Initializing MQTT service...");
            let (mqtt_service, rx) =
                MqttService::new(mqtt_config, db_manager.clone(), device_inbound.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to initialize MQTT service: {}", e))?;

            let mqtt_service_arc = Arc::new(mqtt_service);
            let mqtt_service_for_task = mqtt_service_arc.as_ref().clone();

            info!("MQTT service initialized, starting event loop...");

            // Spawn MQTT service in background
            tokio::spawn(async move {
                if let Err(e) = mqtt_service_for_task.start(rx).await {
                    error!("MQTT service error: {}", e);
                }
            });

            info!("MQTT service started");
            Some(mqtt_service_arc)
        } else {
            warn!("edge apparatuss enabled but no MQTT configuration provided");
            None
        }
    } else {
        info!("edge apparatuss disabled, MQTT service not started");
        None
    };

    // Initialize webhook dispatcher and wire it to audit-log creation.
    info!("Initializing webhook dispatcher...");
    let (webhook_dispatcher, webhook_tx) = WebhookDispatcher::start(db_manager.clone());
    db_manager.set_webhook_sender(webhook_tx);
    info!("Webhook dispatcher initialized");

    // Initialize MFA service (TOTP + WebAuthn).
    info!("Initializing MFA service...");
    let mfa_service = MfaService::new(app_config.auth.mfa.clone());
    info!(
        "MFA service initialized (enabled={}, webauthn_built={})",
        mfa_service.config().enabled,
        mfa_service.webauthn().is_some(),
    );

    // Outbound transport: prefers WS, falls back to MQTT.
    let device_transport = Arc::new(DeviceTransport::new(
        device_registry.clone(),
        mqtt_service_arc.clone(),
    ));

    // Initialize door access service.
    info!("Initializing door access service...");
    let door_service = Arc::new(DoorService::new(
        db_manager.clone(),
        device_transport.clone(),
        app_config.toolguard.profile_field.clone(),
        config_manager.clone(),
    ));
    // Push a fresh state snapshot to every device on startup so an edge
    // restart picks up the current allow-lists.
    if app_config.door.enabled {
        door_service.republish_all();
    }
    info!(
        "Door access service initialized (enabled={})",
        app_config.door.enabled
    );

    // Schedule ticker: re-evaluate every rule's schedule each minute and
    // republish any device whose compiled snapshot changed. Quiet during
    // steady state; fires only on window open/close.
    if app_config.door.enabled {
        let svc = door_service.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                svc.republish_changed();
            }
        });
        info!("Door schedule ticker started (1 minute interval)");
    }

    let app_state = AppState {
        config_manager,
        db: db_manager,
        profile_validator,
        audit_logger,
        throttle_service,
        recaptcha_service,
        calendar_service,
        pages_service,
        mqtt_service: mqtt_service_arc,
        webhook_dispatcher,
        mfa_service,
        door_service,
        device_transport,
        device_registry,
        device_inbound,
    };

    // Serve frontend static files
    let frontend_path = args.frontend_path.clone();
    let serve_dir = ServeDir::new(&frontend_path)
        .not_found_service(ServeFile::new(format!("{}/index.html", frontend_path)));

    let general_route = Router::new().route("/status", get(root));
    // .route("/profile", get(handlers::show_profile))
    // .layer(axum::middleware::from_fn_with_state(
    //     app_state.db.clone(),
    //     web_auth_middleware
    // ));

    let app = Router::new()
        .route("/metrics", get(metrics_handler).with_state(prom.clone()))
        .merge(general_route)
        .nest("/api", api::api_routes().layer(prom.http_layer()))
        .fallback_service(serve_dir)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(&app_config.server.bind_address).await?;
    info!("Server starting on {}", app_config.server.bind_address);
    info!("Site URL: {}", app_config.site.site_url);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Test basic database operations
async fn test_database_operations(db_manager: &DatabaseManager) -> Result<(), anyhow::Error> {
    use css_server::auth::PasswordHashUtil;
    use css_server::models::NewUser;

    // Test database health check
    db_manager.health_check()?;
    info!("Database health check passed");

    // Test user creation
    let hashed_password = PasswordHashUtil::hash("test_password")?;
    let new_user = NewUser::new(
        "test_user".to_string(),
        "test@example.com".to_string(),
        hashed_password,
        "Test User".to_string(),
    );

    // Check if user already exists and clean up if needed
    if let Ok(Some(existing_user)) = db_manager.find_user_by_username("test_user") {
        info!("Cleaning up existing test user");
        db_manager.delete_user(existing_user.id)?;
    }

    // Create test user
    let created_user = db_manager.create_user(&new_user)?;
    info!("Test user created with ID: {}", created_user.id);

    // Test user retrieval
    let found_user = db_manager.find_user_by_id(created_user.id)?;
    assert!(found_user.is_some(), "User should be found");
    info!("Test user found by ID");

    let found_user_by_username = db_manager.find_user_by_username("test_user")?;
    assert!(
        found_user_by_username.is_some(),
        "User should be found by username"
    );
    info!("Test user found by username");

    // Test user count
    let user_count = db_manager.count_users()?;
    info!("Total users in database: {}", user_count);

    let active_user_count = db_manager.count_active_users()?;
    info!("Active users in database: {}", active_user_count);

    // Clean up test user
    db_manager.delete_user(created_user.id)?;
    info!("Test user cleaned up");

    Ok(())
}
