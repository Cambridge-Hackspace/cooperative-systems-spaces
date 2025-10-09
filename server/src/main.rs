use std::sync::Arc;
use axum::extract::{State, FromRef};
use axum::{Json, Router};
use axum::http::StatusCode;
use axum::routing::get;
use clap::Parser;
use serde_json::json;
use tracing::info;
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
use config::{ConfigManager, load_config};
use database::{DatabaseManager, initialize_database};
use profile::{ProfileValidator, AuditLogger};
use throttle::RegistrationThrottleService;
use recaptcha::RecaptchaService;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(long, env = "CONFIG_PATH", default_value = "config.toml")]
    config_path: String,

    /// Generate a sample configuration file and exit
    #[arg(long)]
    generate_config: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub db: Arc<DatabaseManager>,
    pub profile_validator: ProfileValidator,
    pub audit_logger: AuditLogger,
    pub throttle_service: Arc<RegistrationThrottleService>,
    pub recaptcha_service: Arc<RecaptchaService>,
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

    // Setup stats
    //let collector = Collector::default();
    //collector.describe();
    //let (prometheus_layer, _metric_handle) = PrometheusMetricLayer::pair();

    let profile_validator = ProfileValidator::new(&app_config.user);
    let audit_logger = AuditLogger::new(db_manager.clone());
    let throttle_service = Arc::new(RegistrationThrottleService::new());
    let recaptcha_service = Arc::new(RecaptchaService::new(
        app_config.registration_challenge.recaptcha_secret_key.clone()
    ));

    let app_state = AppState {
        config_manager: config_manager,
        db: db_manager,
        profile_validator,
        audit_logger,
        throttle_service,
        recaptcha_service,
    };

    let general_route = Router::new()
        .route("/", get(root));
        // .route("/profile", get(handlers::show_profile))
        // .layer(axum::middleware::from_fn_with_state(
        //     app_state.db.clone(),
        //     web_auth_middleware
        // ));

    let app = Router::new()
        .merge(general_route)
        .nest("/api", api::api_routes())
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
