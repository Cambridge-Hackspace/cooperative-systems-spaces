use std::sync::Arc;
use axum::extract::{State, FromRef};
use axum::{Json, Router};
use axum::http::StatusCode;
use axum::routing::get;
use dr_metrix_axum::{metrics_handler, PrometheusMetrics};
use dr_metrix_core::collector::CollectorConfig;
use dr_metrix_postgres::PostgresMetrics;
use clap::Parser;
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn, error};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod config;
mod database;
mod models;
mod schema;
mod auth;
mod api;
mod profile;
mod throttle;
mod recaptcha;
mod calendar;
mod pages;
mod mqtt;
use config::{ConfigManager, load_config};
use database::{DatabaseManager, initialize_database};
use profile::{ProfileValidator, AuditLogger};
use throttle::RegistrationThrottleService;
use recaptcha::RecaptchaService;
use calendar::CalendarService;
use crate::pages::PagesService;
use crate::mqtt::MqttService;

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

#[derive(Clone)]
pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub db: Arc<DatabaseManager>,
    pub profile_validator: ProfileValidator,
    pub audit_logger: AuditLogger,
    pub throttle_service: Arc<RegistrationThrottleService>,
    pub recaptcha_service: Arc<RecaptchaService>,
    pub calendar_service: Arc<tokio::sync::RwLock<CalendarService>>,
    pub pages_service: Arc<tokio::sync::RwLock<PagesService>>,
    pub mqtt_service: Option<Arc<MqttService>>,
}

// Main dashboard handler
async fn root(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config = state.config_manager.get_config();
    Ok(Json(json!({
        "status": "ok",
        "site_name": config.site.site_name,
        "version": env!("CARGO_PKG_VERSION")
    })))
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
    let app_config = load_config(&args.config_path)?;
    info!("Configuration loaded from: {}", args.config_path);
    info!("Site: {} running in {} mode",
          app_config.site.site_name,
          if app_config.site.debug { "debug" } else { "production" });

    // Create configuration manager
    let config_manager = Arc::new(ConfigManager::new(
        app_config.clone(),
        Some(std::path::PathBuf::from(&args.config_path))
    ));

    // Initialize database
    info!("Initializing database connection pool...");
    let db_manager = Arc::new(initialize_database(&app_config.database).await?);
    info!("Database connection pool initialized successfully");

    // Test basic database operations
    info!("Testing basic database operations...");
    test_database_operations(&db_manager).await?;
    info!("Database operations test completed successfully");

    // Setup Prometheus metrics
    let prom = Arc::new(
        PrometheusMetrics::builder("css")
            .with_process_collector()
            .build()?,
    );
    let pg_config = CollectorConfig { namespace: "css".into(), ..Default::default() };
    let pg_metrics = PostgresMetrics::new(db_manager.pool().clone(), pg_config.clone())?;
    prom.add_collector(pg_metrics, pg_config.collect_interval)?;

    let profile_validator = ProfileValidator::new(&app_config.user);
    let audit_logger = AuditLogger::new(db_manager.clone());
    let throttle_service = Arc::new(RegistrationThrottleService::new());
    let recaptcha_service = Arc::new(RecaptchaService::new(
        app_config.registration_challenge.recaptcha_secret_key.clone()
    ));
    
    // Initialize calendar service
    info!("Initializing calendar service...");
    let calendar_service = Arc::new(tokio::sync::RwLock::new(
        CalendarService::new(app_config.calendar.clone())
    ));
    info!("Calendar service initialized");

    // Initialize pages service
    info!("Initializing pages service...");
    let pages_service = Arc::new(tokio::sync::RwLock::new(
        PagesService::new(app_config.pages.clone()).await?
    ));
    info!("Pages service initialized");

    // Initialize MQTT service if edge is enabled
    let mqtt_service_arc = if app_config.edge.edge_enabled {
        if let Some(mqtt_config) = &app_config.edge.edge_mqtt_config {
            info!("Initializing MQTT service...");
            let (mqtt_service, rx) = MqttService::new(mqtt_config, db_manager.clone())
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

    let app_state = AppState {
        config_manager: config_manager,
        db: db_manager,
        profile_validator,
        audit_logger,
        throttle_service,
        recaptcha_service,
        calendar_service,
        pages_service,
        mqtt_service: mqtt_service_arc,
    };

    // Serve frontend static files
    let frontend_path = args.frontend_path.clone();
    let serve_dir = ServeDir::new(&frontend_path)
        .not_found_service(ServeFile::new(format!("{}/index.html", frontend_path)));

    let general_route = Router::new()
        .route("/status", get(root));
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
    use models::NewUser;
    use auth::PasswordHashUtil;

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
    assert!(found_user_by_username.is_some(), "User should be found by username");
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
