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
use css_server::groupsio::GroupsioClient;
use css_server::groupsio_sync::GroupsIoService;
use css_server::mail::MailService;
use css_server::membership::MembershipService;
use css_server::mfa::MfaService;
use css_server::mqtt::MqttService;
use css_server::pages::PagesService;
use css_server::profile::AuditLogger;
use css_server::profile_fields;
use css_server::recaptcha::RecaptchaService;
use css_server::stripe::StripeClient;
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
            let profiles_enabled = latest.profiles_enabled.unwrap_or_else(|| {
                warn!(
                    "profile_config_versions version {} predates the profiles_enabled column; \
                     falling back to config file's profiles_enabled_seed",
                    latest.version
                );
                app_config.user.profiles_enabled_seed
            });
            config_manager.set_profiles_enabled(profiles_enabled);
            info!(
                "Loaded profile field schema from database (version {})",
                latest.version
            );
        }
        None => {
            profile_fields::validate_profile_fields(&app_config.user.profile_fields_seed).map_err(
                |e| anyhow::anyhow!("Invalid profile_fields_seed in config file: {}", e),
            )?;
            let seed = serde_json::to_value(&app_config.user.profile_fields_seed)?;
            db_manager.insert_profile_config_version(
                seed,
                app_config.user.profiles_enabled_seed,
                None,
            )?;
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

    let audit_logger = AuditLogger::new(db_manager.clone());
    let throttle_service = Arc::new(RegistrationThrottleService::new());
    let mail_service = Arc::new(MailService::new(config_manager.clone()));
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
            let (mqtt_service, rx) = MqttService::new(mqtt_config, device_inbound.clone())
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
    let (webhook_dispatcher, webhook_tx) =
        WebhookDispatcher::start(db_manager.clone(), config_manager.clone());
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

    // Groups.io mailing-list sync. Only wired when the module is enabled: build
    // the client and service, register its sender as a second consumer of the
    // audit-event fan-out (alongside the webhook dispatcher), and spawn the
    // reconciliation ticker -- the same idiom as the door ticker above.
    let groupsio_sync = if app_config.groupsio.enabled {
        info!("Initializing Groups.io mailing-list sync...");
        let client = GroupsioClient::new(config_manager.clone());
        let (svc, groupsio_tx) =
            GroupsIoService::start(db_manager.clone(), config_manager.clone(), client);
        db_manager.set_groupsio_sender(groupsio_tx);

        let interval_secs = app_config.groupsio.sync_interval_secs.max(1);
        let recon = svc.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let outcome = recon.reconcile_once().await;
                if !outcome.ok {
                    if let Some(err) = &outcome.error {
                        warn!("Groups.io reconciliation did not complete: {err}");
                    }
                } else if outcome.added + outcome.removed + outcome.opted_out > 0 {
                    info!(
                        "Groups.io reconciled: +{} -{} opt-out {}",
                        outcome.added, outcome.removed, outcome.opted_out
                    );
                }
            }
        });
        info!(
            "Groups.io mailing-list sync started (reconcile every {}s)",
            interval_secs
        );
        Some(svc)
    } else {
        info!("Groups.io integration disabled, mailing-list sync not started");
        None
    };

    // Membership dues-ledger service. Built when the module is enabled (Stripe is
    // an optional payment layer on top -- absent it, the ledger runs cash-only).
    // The daily renewal ticker follows the same interval idiom as the door and
    // groups.io tickers above.
    let membership = if app_config.membership.enabled {
        info!("Initializing membership billing...");
        let stripe = if app_config.stripe.enabled {
            Some(StripeClient::new(config_manager.clone()))
        } else {
            None
        };
        let svc = MembershipService::new(
            db_manager.clone(),
            config_manager.clone(),
            door_service.clone(),
            stripe,
        );

        let interval_secs = app_config.membership.accrual_interval_secs.max(1);
        let cycle = svc.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let outcome = cycle.run_cycle().await;
                if !outcome.ok {
                    if let Some(err) = &outcome.error {
                        warn!("Membership renewal cycle did not complete: {err}");
                    }
                } else if outcome.dues_charged + outcome.lapsed + outcome.credited > 0 {
                    info!(
                        "Membership cycle: {} checked, {} dues, {} lapsed, {} credited",
                        outcome.users_checked,
                        outcome.dues_charged,
                        outcome.lapsed,
                        outcome.credited
                    );
                }
            }
        });
        info!(
            "Membership billing started (renewal cycle every {}s, stripe {})",
            interval_secs,
            if app_config.stripe.enabled {
                "enabled"
            } else {
                "disabled (cash-only)"
            }
        );
        Some(svc)
    } else {
        info!("Membership module disabled, billing not started");
        None
    };

    let app_state = AppState {
        config_manager,
        db: db_manager,
        audit_logger,
        throttle_service,
        recaptcha_service,
        mail_service,
        calendar_service,
        pages_service,
        mqtt_service: mqtt_service_arc,
        webhook_dispatcher,
        mfa_service,
        door_service,
        device_transport,
        device_registry,
        device_inbound,
        groupsio_sync,
        membership,
    };

    // Serve frontend static files
    let frontend_path = args.frontend_path.clone();
    // `fallback`, not `not_found_service`. They differ in exactly one way and it
    // is the one that matters here: tower-http's `not_found_service` is
    // documented as "set the fallback service **and override the fallback's
    // status code to 404 Not Found**". So every client-side route -- /tools,
    // /profile, /door/{id}/checkin -- was served the correct index.html with a
    // 404 status.
    //
    // In a browser that is invisible: the body arrives, Vue Router takes over,
    // the page works. Everywhere else it is not. Uptime monitoring reports
    // every deep link as missing. Anything checking `res.ok` treats the page as
    // an error. Link previews and crawlers see a dead URL. And the concrete
    // one for this product: /door/{id}/checkin is a QR code on a door, opened
    // cold by a phone camera, and some in-app browsers and link scanners refuse
    // or warn on a 404 before a person ever sees the page.
    //
    // Found by the stack battery probing the fallback at three depths rather
    // than one. / was 200 because ServeDir found index.html for itself and the
    // fallback never ran.
    let serve_dir = ServeDir::new(&frontend_path)
        .fallback(ServeFile::new(format!("{}/index.html", frontend_path)));

    let general_route = Router::new().route("/status", get(root));
    // .route("/profile", get(handlers::show_profile))
    // .layer(axum::middleware::from_fn_with_state(
    //     app_state.db.clone(),
    //     web_auth_middleware
    // ));

    // CORS applies to the /api nest and nowhere else: it is the only
    // cross-origin surface a browser calls, and the SPA, /status and /metrics
    // are served same-origin. `build_layer` returns None when cors_enabled is
    // false -- the honest form of the pre-existing default, no layer and no
    // headers -- and `option_layer` makes that None a no-op. It is layered
    // inline rather than through a `let api = ...` so the nest stays literally
    // `.nest("/api", api::api_routes()...)`, which the contract-tier parity
    // check pins (main_composes_the_same_router). An invalid configuration was
    // already refused by validate_config at load; the `?` here covers the
    // reload path and keeps this total. See css_server::cors.
    let cors = css_server::cors::build_layer(&app_config.server)?;

    // The SPA fallback below catches everything the router did not match --
    // including, without care, unmatched /api paths. `api::api_routes()` owns
    // its own 404 for exactly that reason; the note is there.
    let app = Router::new()
        .route("/metrics", get(metrics_handler).with_state(prom.clone()))
        .merge(general_route)
        .nest(
            "/api",
            api::api_routes()
                .layer(prom.http_layer())
                .layer(tower::util::option_layer(cors)),
        )
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
