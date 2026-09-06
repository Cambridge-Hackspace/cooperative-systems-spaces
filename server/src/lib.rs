//! `css-server` — the CSS instance: HTTP API, database, MQTT, doors, training.
//!
//! Everything lives in the library and `main.rs` is a thin shim over it. That
//! is not cosmetic. Until this split all eighteen modules were declared
//! privately in `main.rs`, which meant:
//!
//! * `server/tests/` could reach nothing at all, so the entire server-contract
//!   tier — the authorization matrix over every route × every credential
//!   state — was structurally impossible to write;
//! * a large amount of live code was reported as dead, because a binary's
//!   private items are dead unless the binary itself calls them.
//!
//! Every module is `pub`, and that is deliberate rather than lazy. [`AppState`]
//! is `pub` with fifteen `pub` fields whose types come from thirteen of these
//! modules; a `pub` field of a `pub(crate)` type trips the `private_interfaces`
//! lint, which is a hard error under `-D warnings`. `schema` in particular must
//! be public because `models/*.rs` names `crate::schema::sql_types::*` inside
//! `#[diesel(...)]` attributes. This library is never published, and pretending
//! to a narrow surface while every field is reachable through `AppState` would
//! be theatre.
//!
//! What stays in the binary: argument parsing, tracing setup, the boot
//! sequence, and the Prometheus wiring. The `dr-metrix` crates are Linux-only
//! (they call `prometheus::process_collector`, which is gated on
//! `target_os = "linux"`), and keeping them out of the library keeps that
//! constraint in one place.

use std::sync::Arc;

pub mod api;
pub mod auth;
pub mod calendar;
pub mod config;
pub mod cors;
pub mod database;
pub mod devices_inbound;
pub mod devices_transport;
pub mod doors;
pub mod groupsio;
pub mod mail;
pub mod mfa;
pub mod models;
pub mod mqtt;
pub mod pages;
pub mod profile;
pub mod profile_fields;
pub mod recaptcha;
pub mod schedules;
pub mod schema;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod throttle;
pub mod tokens;
pub mod webhooks;

use crate::calendar::CalendarService;
use crate::config::ConfigManager;
use crate::database::DatabaseManager;
use crate::devices_inbound::DeviceInbound;
use crate::devices_transport::{DeviceChannelRegistry, DeviceTransport};
use crate::doors::DoorService;
use crate::mail::MailService;
use crate::mfa::MfaService;
use crate::mqtt::MqttService;
use crate::pages::PagesService;
use crate::profile::AuditLogger;
use crate::recaptcha::RecaptchaService;
use crate::throttle::RegistrationThrottleService;
use crate::webhooks::WebhookDispatcher;

#[derive(Clone)]
pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub db: Arc<DatabaseManager>,
    pub audit_logger: AuditLogger,
    pub throttle_service: Arc<RegistrationThrottleService>,
    pub recaptcha_service: Arc<RecaptchaService>,
    /// Outbound SMTP. Reads `[email]` live, so a reloaded config takes effect
    /// on the next send rather than at the next restart.
    pub mail_service: Arc<MailService>,
    pub calendar_service: Arc<tokio::sync::RwLock<CalendarService>>,
    pub pages_service: Arc<tokio::sync::RwLock<PagesService>>,
    pub mqtt_service: Option<Arc<MqttService>>,
    pub webhook_dispatcher: Arc<WebhookDispatcher>,
    pub mfa_service: MfaService,
    pub door_service: Arc<DoorService>,
    /// Transport-agnostic outbound router (WS → MQTT fallback).
    pub device_transport: Arc<DeviceTransport>,
    /// Per-device WS session registry; the WS handler inserts/removes itself.
    pub device_registry: Arc<DeviceChannelRegistry>,
    /// Shared inbound dispatcher used by both transports.
    pub device_inbound: Arc<DeviceInbound>,
}

/// `GET /status` — the JSON liveness handler.
///
/// Lives here rather than in the binary because it is the one route a client
/// can rely on without authenticating, and `css-cli health` and `css-cli info`
/// both call it. (They used to call `/`, which the static-file fallback serves
/// as `index.html` — so `info` was parsing HTML as JSON.)
pub async fn root(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    let config = state.config_manager.get_config();
    Ok(axum::Json(serde_json::json!({
        "status": "ok",
        "site_name": config.site.site_name,
        "version": env!("CARGO_PKG_VERSION")
    })))
}
