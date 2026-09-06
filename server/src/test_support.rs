//! Fixtures for the server-contract tier.
//!
//! Compiled only under `cfg(test)` or the `test-support` feature, which is
//! never enabled by default — so none of this can reach a shipped binary.
//!
//! What this buys: an [`AppState`] with no database behind it, so the entire
//! request-*rejection* surface can be asserted in-process. That is most of the
//! server's security-relevant behavior, and none of it reaches the database:
//! `AuthUser::from_request_parts` checks the header, the `Bearer` prefix and
//! the JWT signature before its single `find_user_by_id`.
//!
//! Read [`DatabaseManager::disconnected`] before writing a test against this.
//! The short version: **500 means you reached the dead pool**, it is distinct
//! from every legitimate rejection, and a test that accepts one as a result is
//! testing nothing.

use std::sync::Arc;

use crate::calendar::CalendarService;
use crate::config::{AppConfig, ConfigManager};
use crate::database::DatabaseManager;
use crate::devices_inbound::DeviceInbound;
use crate::devices_transport::{DeviceChannelRegistry, DeviceTransport};
use crate::doors::DoorService;
use crate::mfa::MfaService;
use crate::pages::PagesService;
use crate::profile::AuditLogger;
use crate::recaptcha::RecaptchaService;
use crate::throttle::RegistrationThrottleService;
use crate::webhooks::WebhookDispatcher;
use crate::AppState;

/// The signing secret every fixture token is minted with.
pub const TEST_JWT_SECRET: &str = "css-test-secret-not-for-any-real-deployment";

/// A configuration safe to build a fixture from.
///
/// `AppConfig::default()` is **not** safe on its own, and the reason is worth
/// stating: `PagesConfig::default()` carries live GitHub URLs, and
/// `PagesService::new` shells out to `git clone` into the hardcoded paths
/// `/tmp/css-wiki-repo` and `/tmp/css-site-repo`. A fixture built naively from
/// the defaults therefore performs two network clones into a shared location
/// from every test binary, in parallel — which is slow, flaky, and reaches the
/// network from a unit test.
pub fn test_config() -> AppConfig {
    let mut config = AppConfig::default();

    config.auth.jwt_secret = TEST_JWT_SECRET.to_string();
    config.pages.wiki_repo = None;
    config.pages.site_repo = None;

    assert!(
        config.pages.wiki_repo.is_none() && config.pages.site_repo.is_none(),
        "the pages repos must be None: PagesService::new git-clones them into a \
         shared /tmp path, so a fixture that leaves them set reaches the network"
    );

    assert!(
        !config.email.enabled,
        "email must stay disabled: MailService opens an SMTP connection on every \
         send, so a fixture that enables it reaches the network -- and would do \
         so from the offline contract tier, whose whole premise is that nothing \
         it touches can talk to anything"
    );

    config
}

/// An [`AppState`] with a non-connecting database.
///
/// Async because [`PagesService::new`] is, and because `WebhookDispatcher::start`
/// spawns a background task.
pub async fn app_state() -> AppState {
    let config = test_config();
    let config_manager = Arc::new(ConfigManager::new(config.clone(), None));
    let db = Arc::new(DatabaseManager::disconnected());

    let (webhook_dispatcher, _audit_tx) =
        WebhookDispatcher::start(db.clone(), config_manager.clone());

    let device_registry = DeviceChannelRegistry::new();
    let device_transport = Arc::new(DeviceTransport::new(device_registry.clone(), None));

    let pages_service = PagesService::new(config.pages.clone())
        .await
        .expect("PagesService::new with both repos None does no I/O");

    AppState {
        audit_logger: AuditLogger::new(db.clone()),
        throttle_service: Arc::new(RegistrationThrottleService::new()),
        recaptcha_service: Arc::new(RecaptchaService::new(String::new())),
        // `test_config()` leaves `email.enabled` false, so no fixture can reach
        // the network through this. Asserted below rather than assumed.
        mail_service: Arc::new(crate::mail::MailService::new(config_manager.clone())),
        calendar_service: Arc::new(tokio::sync::RwLock::new(CalendarService::new(
            config.calendar.clone(),
        ))),
        pages_service: Arc::new(tokio::sync::RwLock::new(pages_service)),
        mqtt_service: None,
        webhook_dispatcher,
        mfa_service: MfaService::new(config.auth.mfa.clone()),
        door_service: Arc::new(DoorService::new(
            db.clone(),
            device_transport.clone(),
            config.toolguard.profile_field.clone(),
            config_manager.clone(),
        )),
        device_inbound: Arc::new(DeviceInbound::new(
            db.clone(),
            config.toolguard.profile_field.clone(),
        )),
        device_transport,
        device_registry,
        db,
        config_manager,
        // The mailing-list sync is wired only when the module is enabled;
        // fixtures leave it off, and reconcile-now checks for it before running.
        groupsio_sync: None,
    }
}
