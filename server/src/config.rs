use anyhow::{Context, Result};
pub use css_lib::MqttConfig;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// Merge two TOML values, with the first taking precedence
/// This is used to merge existing config with defaults for missing fields
fn merge_toml_values(existing: toml::Value, defaults: toml::Value) -> toml::Value {
    use toml::Value;

    match (existing, defaults) {
        (Value::Table(mut existing_table), Value::Table(defaults_table)) => {
            // For tables, merge recursively
            for (key, default_value) in defaults_table {
                if let Some(existing_value) = existing_table.remove(&key) {
                    // Key exists, merge recursively
                    existing_table.insert(key, merge_toml_values(existing_value, default_value));
                } else {
                    // Key missing, use default
                    existing_table.insert(key, default_value);
                }
            }
            Value::Table(existing_table)
        }
        (existing_val, _) => {
            // For non-tables, existing value takes precedence
            existing_val
        }
    }
}

/// Toggles for the built-in home-page action buttons (View My Profile,
/// Browse Tools, Admin Panel, Wiki). Operator-curated entries from the
/// `home_links` admin page are unaffected.
///
/// The `wiki` flag is combined with the existing `[pages]` settings: the
/// button only appears when wiki is configured server-side **and** its
/// `wiki_link` is set to `HomePage` or `Both` **and** this toggle is true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomepageLinksConfig {
    pub view_my_profile: bool,
    pub browse_tools: bool,
    pub admin_panel: bool,
    pub wiki: bool,
}

impl Default for HomepageLinksConfig {
    fn default() -> Self {
        Self {
            view_my_profile: true,
            browse_tools: true,
            admin_panel: true,
            wiki: true,
        }
    }
}

/// Site-specific configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    /// The name of the site
    pub site_name: String,
    /// The main URL of the site (including protocol)
    pub site_url: String,
    /// URL for the admin panel
    pub admin_url: String,
    /// Default timezone for the site
    pub timezone: String,
    /// Maximum duration for inactive sessions (in minutes)
    pub max_session_age: i32,
    /// Enable debug mode
    pub debug: bool,
    /// Secret key for cryptographic signing
    pub secret_key: String,
    /// Enable HTTPS enforcement
    pub https: bool,
    /// Analytics tracking ID (optional)
    pub analytics_id: Option<String>,
    /// Which built-in homepage buttons are shown. Each defaults to `true`.
    #[serde(default)]
    pub homepage_links: HomepageLinksConfig,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            site_name: "Cooperative Systems Spaces".to_string(),
            site_url: "http://localhost:4399".to_string(),
            admin_url: "http://localhost:4399/admin".to_string(),
            timezone: "UTC".to_string(),
            max_session_age: 1440, // 24 hours
            debug: false,
            secret_key: "change-me-in-production".to_string(),
            https: false,
            analytics_id: None,
            homepage_links: HomepageLinksConfig::default(),
        }
    }
}

/// Email server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    /// SMTP server hostname
    pub host: String,
    /// SMTP server port
    pub port: u16,
    /// Username for SMTP authentication
    pub username: String,
    /// Password for SMTP authentication
    pub password: String,
    /// Use TLS encryption
    pub use_tls: bool,
    /// Use SSL encryption
    pub use_ssl: bool,
    /// Default from email address
    pub from_email: String,
    /// Display name for the from address
    pub from_name: String,
    /// Enable email functionality
    pub enabled: bool,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 587,
            username: String::new(),
            password: String::new(),
            use_tls: true,
            use_ssl: false,
            from_email: "noreply@example.com".to_string(),
            from_name: "Cooperative Systems Spaces".to_string(),
            enabled: false,
        }
    }
}

/// Theme and UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Primary color for the theme
    pub primary_color: String,
    /// Secondary color for the theme
    pub secondary_color: String,
    /// Accent color for the theme
    pub accent_color: String,
    /// Background color
    pub background_color: String,
    /// Text color
    pub text_color: String,
    /// Custom CSS file path
    pub custom_css_file: Option<String>,
    /// Logo URL or path
    pub logo_url: Option<String>,
    /// Favicon URL or path
    pub favicon_url: Option<String>,
    /// Enable dark mode support
    pub dark_mode_enabled: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            primary_color: "#007bff".to_string(),
            secondary_color: "#6c757d".to_string(),
            accent_color: "#28a745".to_string(),
            background_color: "#ffffff".to_string(),
            text_color: "#212529".to_string(),
            custom_css_file: None,
            logo_url: None,
            favicon_url: None,
            dark_mode_enabled: true,
        }
    }
}

/// Stripe payment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StripeConfig {
    /// Stripe publishable key
    pub publishable_key: String,
    /// Stripe secret key
    pub secret_key: String,
    /// Webhook endpoint secret
    pub webhook_secret: String,
    /// Enable Stripe integration
    pub enabled: bool,
    /// Default currency code
    pub currency: String,
    /// Test mode (use test keys)
    pub test_mode: bool,
}

impl Default for StripeConfig {
    fn default() -> Self {
        Self {
            publishable_key: String::new(),
            secret_key: String::new(),
            webhook_secret: String::new(),
            enabled: false,
            currency: "USD".to_string(),
            test_mode: true,
        }
    }
}

/// Reporting and analytics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    /// Enable usage reports
    pub usage_reports_enabled: bool,
    /// Enable member reports
    pub member_reports_enabled: bool,
    /// Enable financial reports
    pub financial_reports_enabled: bool,
    /// Report generation frequency in hours
    pub generation_frequency_hours: u32,
    /// Maximum number of reports to retain
    pub max_reports_retained: u32,
    /// Export format for reports
    pub default_export_format: String,
    /// Email reports to administrators
    pub email_reports: bool,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            usage_reports_enabled: true,
            member_reports_enabled: true,
            financial_reports_enabled: false,
            generation_frequency_hours: 24,
            max_reports_retained: 30,
            default_export_format: "csv".to_string(),
            email_reports: false,
        }
    }
}

/// Space directory and mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceDirectoryConfig {
    /// Enable space directory functionality
    pub enabled: bool,
    /// Default space coordinates format
    pub coordinate_format: String,
    /// Maximum number of spaces per directory
    pub max_spaces_per_directory: u32,
    /// Enable space search functionality
    pub search_enabled: bool,
    /// Enable space filtering
    pub filtering_enabled: bool,
    /// Default space visibility
    pub default_visibility: String,
    /// Allow anonymous space viewing
    pub allow_anonymous_viewing: bool,
}

impl Default for SpaceDirectoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            coordinate_format: "decimal".to_string(),
            max_spaces_per_directory: 1000,
            search_enabled: true,
            filtering_enabled: true,
            default_visibility: "public".to_string(),
            allow_anonymous_viewing: true,
        }
    }
}

/// Sentry error tracking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryConfig {
    /// Sentry DSN for error tracking
    pub dsn: Option<String>,
    /// Environment name for Sentry
    pub environment: String,
    /// Sample rate for performance monitoring
    pub traces_sample_rate: f64,
    /// Enable Sentry integration
    pub enabled: bool,
    /// Enable performance monitoring
    pub performance_monitoring: bool,
    /// Release version for Sentry
    pub release: Option<String>,
}

impl Default for SentryConfig {
    fn default() -> Self {
        Self {
            dsn: None,
            environment: "development".to_string(),
            traces_sample_rate: 0.1,
            enabled: false,
            performance_monitoring: false,
            release: None,
        }
    }
}

/// Database connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database host
    pub host: String,
    /// Database port
    pub port: u16,
    /// Database name
    pub database: String,
    /// Database username
    pub username: String,
    /// Database password
    pub password: String,
    /// Full database connection URL (will be constructed if not provided)
    pub url: Option<String>,
    /// Maximum number of connections in the pool
    pub max_connections: u32,
    /// Minimum number of connections in the pool
    pub min_connections: u32,
    /// Connection timeout in seconds
    pub connect_timeout_seconds: u64,
    /// Idle timeout in seconds
    pub idle_timeout_seconds: u64,
    /// Enable connection pool logging
    pub log_statements: bool,
    /// Enable database migrations on startup
    pub auto_migrate: bool,
}

/// Authentication configuration
/// Registration challenge configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationChallengeConfig {
    /// Enable the registration challenge requirement
    pub enabled: bool,
    /// Hint to display to users about what the challenge might be
    pub hint: String,
    /// The exact phrase users must enter (case-sensitive)
    pub phrase: String,
    /// Enable throttling of failed attempts
    pub throttle_enabled: bool,
    /// The number of attempts allowed before the user is locked out
    pub throttle_attempts: u32,
    /// The number of seconds before the user is allowed to retry,
    pub throttle_seconds: u32,
    /// Enable the terms of service checkbox
    pub terms_of_service_checkbox: bool,
    /// Terms of service markdown text to display to users
    pub terms_of_service_md: String,
    /// Enable ReCaptcha-Style Challenges
    pub recaptcha_enabled: bool,
    /// reCAPTCHA site key
    pub recaptcha_site_key: String,
    /// reCAPTCHA secret key
    pub recaptcha_secret_key: String,
}

impl Default for RegistrationChallengeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hint: "Ask a current member for the registration phrase".to_string(),
            phrase: "makers welcome here".to_string(),
            throttle_enabled: true,
            throttle_attempts: 5,
            throttle_seconds: 300,
            terms_of_service_checkbox: true,
            terms_of_service_md: "By registering, you agree to the <a href=\"/terms\" target=\"_blank\" class=\"link link-primary\">terms of service</a>".to_string(),
            recaptcha_enabled: false,
            recaptcha_site_key: String::new(),
            recaptcha_secret_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// JWT secret key for signing tokens.
    ///
    /// Optional in the file. `config.sample.toml` deliberately ships without
    /// it -- writing a real secret into a sample is how a public one ends up
    /// signing production tokens -- so this has to deserialize from its
    /// absence or the shipped sample does not parse at all.
    ///
    /// Absence means the empty string here, not a generated secret, and that
    /// is deliberate: `load_config` fills it in *and writes it back*.
    /// Generating in the serde default would mint a fresh secret on every
    /// boot without persisting any of them, so every restart would invalidate
    /// every session -- a symptom that looks like broken token expiry and
    /// would not lead anyone here. `validate_config` refuses an empty secret,
    /// so the unfilled value can never reach `EncodingKey`.
    #[serde(default)]
    pub jwt_secret: String,
    /// JWT token expiration time in hours
    pub jwt_expiration_hours: u32,
    /// Enable user registration
    pub allow_registration: bool,
    /// Require email verification
    pub require_email_verification: bool,
    /// Password minimum length
    pub password_min_length: usize,
    /// Session timeout in minutes
    pub session_timeout_minutes: u32,
    /// Enable password reset functionality
    pub password_reset_enabled: bool,
    /// Multi-factor authentication settings
    #[serde(default)]
    pub mfa: AuthMfaConfig,
}

/// Who is required to enroll in MFA before they can use the system fully.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MfaEnforcement {
    /// Users may opt in; never blocks unenrolled users.
    OptIn,
    /// Staff and Admin must enroll. Members/Newbies remain opt-in.
    RequiredForStaff,
    /// All users must enroll on next login.
    RequiredForAll,
}

impl Default for MfaEnforcement {
    fn default() -> Self {
        Self::OptIn
    }
}

/// Multi-factor authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMfaConfig {
    /// Master toggle. When false, MFA is unavailable regardless of other fields.
    pub enabled: bool,
    /// Who must enroll.
    pub enforcement: MfaEnforcement,
    /// Whether TOTP (authenticator app) is offered.
    pub allow_totp: bool,
    /// Whether WebAuthn (Yubikey / Touch ID / passkey) is offered.
    pub allow_webauthn: bool,
    /// Issuer shown in TOTP app entries (and in the otpauth URI).
    pub issuer: String,
    /// How many recovery codes to generate per user.
    pub recovery_code_count: u32,
    /// WebAuthn Relying Party ID (host, no scheme/port). Example: "example.org".
    pub relying_party_id: String,
    /// Human-readable Relying Party name shown by the authenticator.
    pub relying_party_name: String,
    /// Full origin (scheme + host + optional port) the browser will report.
    /// Example: "https://example.org".
    pub relying_party_origin: String,
}

impl Default for AuthMfaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            enforcement: MfaEnforcement::OptIn,
            allow_totp: true,
            allow_webauthn: true,
            issuer: "Cooperative Systems Spaces".to_string(),
            recovery_code_count: 10,
            relying_party_id: "localhost".to_string(),
            relying_party_name: "Cooperative Systems Spaces".to_string(),
            relying_party_origin: "http://localhost:3000".to_string(),
        }
    }
}

impl AuthMfaConfig {
    /// True when the given role must enroll under this enforcement setting.
    pub fn is_required_for(&self, role: &crate::models::UserRole) -> bool {
        if !self.enabled {
            return false;
        }
        use crate::models::UserRole::*;
        match self.enforcement {
            MfaEnforcement::OptIn => false,
            MfaEnforcement::RequiredForStaff => matches!(role, Staff | Admin),
            MfaEnforcement::RequiredForAll => true,
        }
    }
}

/// The old, publicly-known placeholder JWT secret. Any config carrying this
/// exact value (rather than a generated one) signing tokens is a full auth
/// bypass, so it's rejected at load time rather than just warned about.
const LEGACY_DEFAULT_JWT_SECRET: &str = "your-super-secret-jwt-key-change-this-in-production";

/// A fresh, unguessable JWT secret for new installs: a recognizable prefix
/// (so it's still obvious in a config file that it was auto-generated,
/// not a deliberately chosen production secret) plus 32 random
/// alphanumeric characters, which is enough entropy for an HMAC key.
fn generate_default_jwt_secret() -> String {
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    format!("{}-{}", LEGACY_DEFAULT_JWT_SECRET, suffix)
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: generate_default_jwt_secret(),
            jwt_expiration_hours: 24,
            allow_registration: true,
            require_email_verification: false,
            password_min_length: 8,
            session_timeout_minutes: 1440, // 24 hours
            password_reset_enabled: true,
            mfa: AuthMfaConfig::default(),
        }
    }
}

/// Initial setup configuration for first-time deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialSetupConfig {
    /// Enable initial setup mode - first user with matching email gets admin role
    pub setup_enabled: bool,
    /// Email address of the intended first admin user
    pub setup_admin_email: String,
}

impl Default for InitialSetupConfig {
    fn default() -> Self {
        Self {
            setup_enabled: true,
            setup_admin_email: "admin@example.com".to_string(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "css".to_string(),
            username: "css_user".to_string(),
            password: "css_pass".to_string(),
            url: None,
            max_connections: 10,
            min_connections: 1,
            connect_timeout_seconds: 30,
            idle_timeout_seconds: 600,
            log_statements: false,
            auto_migrate: true,
        }
    }
}

impl DatabaseConfig {
    /// Get the database URL, either from the configured URL or construct it from components
    pub fn get_url(&self) -> String {
        match &self.url {
            Some(url) => url.clone(),
            None => format!(
                "postgresql://{}:{}@{}:{}/{}",
                self.username, self.password, self.host, self.port, self.database
            ),
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server bind address
    pub bind_address: String,
    /// Enable request logging
    pub log_requests: bool,
    /// Request timeout in seconds
    pub request_timeout_seconds: u64,
    /// Maximum request body size in bytes
    pub max_request_body_size: usize,
    /// Enable CORS
    pub cors_enabled: bool,
    /// Allowed CORS origins
    pub cors_origins: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:4399".to_string(),
            log_requests: true,
            request_timeout_seconds: 30,
            max_request_body_size: 16 * 1024 * 1024, // 16MB
            cors_enabled: true,
            cors_origins: vec!["http://localhost:3000".to_string()],
        }
    }
}

/// Profile field configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileField {
    /// Field name/key
    pub key: String,
    /// Human readable label
    pub label: String,
    /// Field type
    pub field_type: ProfileFieldType,
    /// Is this field required
    pub required: bool,
    /// Help text for the field
    pub help_text: Option<String>,
}

/// User profile field configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    /// Initial profile field definitions, used only to seed the database's
    /// versioned `profile_config_versions` table on first boot. Once any
    /// version exists there, the database is authoritative and this value
    /// is ignored — editing it in config.toml after first boot has no
    /// effect. See `api::profiles::current_profile_config`.
    pub profile_fields_seed: Vec<ProfileField>,
    /// Initial profiles-enabled toggle, used only to seed the database on
    /// first boot. Same caveat as `profile_fields_seed`.
    pub profiles_enabled_seed: bool,
}

/// Profile field type specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileFieldType {
    Text,
    /// An ordered list of free-form text values (e.g. multiple RFID card IDs
    /// per user). Stored as a JSON array of strings in `profile`.
    TextArray,
    Email,
    Phone,
    Number,
    Date,
    Boolean,
    Select {
        options: Vec<String>,
    },
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            profile_fields_seed: vec![
                ProfileField {
                    key: "bio".to_string(),
                    label: "Bio".to_string(),
                    field_type: ProfileFieldType::Text,
                    required: false,
                    help_text: Some("Tell us about yourself".to_string()),
                },
                ProfileField {
                    key: "phone".to_string(),
                    label: "Phone Number".to_string(),
                    field_type: ProfileFieldType::Phone,
                    required: false,
                    help_text: Some("Your contact phone number".to_string()),
                },
                ProfileField {
                    key: "emergency_contact".to_string(),
                    label: "Emergency Contact".to_string(),
                    field_type: ProfileFieldType::Text,
                    required: false,
                    help_text: Some("Emergency contact information".to_string()),
                },
            ],
            profiles_enabled_seed: true,
        }
    }
}

/// ToolGuard Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGuardConfig {
    /// is it enabled?
    pub enabled: bool,
    /// what profile field we should pull this out of
    pub profile_field: String,
    /// global-api-key
    pub global_api_key: Option<String>,
}

impl Default for ToolGuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_field: "card_id".to_string(),
            global_api_key: None,
        }
    }
}

/// Configurable hierarchy of physical places (`Building → Floor → Room → …`).
/// The level vocabulary is operator-defined; children must use a level
/// strictly deeper than their parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceConfig {
    /// Master toggle. When false, the places admin/UI is hidden and the
    /// optional `place_id` columns on doors / tools / devices are ignored.
    pub enabled: bool,
    /// Ordered list of place-type names, from most-containing to least.
    /// Index in this list = depth.
    pub types: Vec<String>,
}

impl Default for PlaceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // Two-element default so the out-of-the-box experience
            // demonstrates the parent → child relationship (a Room
            // containing a Spot) rather than a degenerate single-level
            // setup.
            types: vec!["Room".to_string(), "Spot".to_string()],
        }
    }
}

impl PlaceConfig {
    /// Depth of a given place type, or `None` if it isn't configured.
    pub fn index_of(&self, place_type: &str) -> Option<usize> {
        self.types.iter().position(|t| t == place_type)
    }

    /// `Ok(())` iff `child_type` is deeper than `parent_type`. Both types
    /// must appear in [`Self::types`].
    pub fn validate_parent_child(&self, parent_type: &str, child_type: &str) -> Result<(), String> {
        let parent_idx = self
            .index_of(parent_type)
            .ok_or_else(|| format!("Unknown parent place_type '{parent_type}'"))?;
        let child_idx = self
            .index_of(child_type)
            .ok_or_else(|| format!("Unknown place_type '{child_type}'"))?;
        if child_idx <= parent_idx {
            return Err(format!(
                "Child place_type '{child_type}' must be deeper than parent '{parent_type}'"
            ));
        }
        Ok(())
    }
}

/// Door access module configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoorConfig {
    /// Master toggle. When false, all door endpoints reject and no MQTT
    /// state is published.
    pub enabled: bool,
    /// Default unlock pulse length (ms) for newly-created doors.
    pub default_unlock_duration_ms: i32,
    /// Public URL placed onto door signage. `{site_url}` and `{door_id}` are
    /// interpolated when an admin requests the QR.
    pub qr_url_template: String,
}

impl Default for DoorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_unlock_duration_ms: 5000,
            qr_url_template: "{site_url}/door/{door_id}/checkin".to_string(),
        }
    }
}

/// Calendar aggregation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarConfig {
    /// Enable calendar display on home page
    pub enabled: bool,
    /// List of calendar sources
    pub calendars: Vec<CalendarSource>,
    /// Cache duration in minutes
    pub cache_duration_minutes: u64,
    /// Maximum number of events to display
    pub max_events_display: usize,
    /// Number of days to look ahead for events
    pub lookahead_days: i64,
}

/// Individual calendar source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarSource {
    /// iCal/ICS feed URL
    pub ical_link: String,
    /// Display name for this calendar
    pub name: String,
    /// Color for calendar events (hex format, e.g., "#FF5733")
    pub color: String,
    /// Whether this calendar is enabled
    pub enabled: bool,
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            calendars: vec![CalendarSource {
                ical_link: "https://example.com/calendar.ics".to_string(),
                name: "Example Calendar".to_string(),
                color: "#3788d8".to_string(),
                enabled: false,
            }],
            cache_duration_minutes: 15,
            max_events_display: 10,
            lookahead_days: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LinkLocation {
    Navigation,
    HomePage,
    Both,
}

/// Configuration for git-pages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PagesConfig {
    /// Path to the Wiki Git Repository
    pub wiki_repo: Option<String>,
    /// Link on the ...
    pub wiki_link: LinkLocation,
    /// Do we run the wiki update & polling system
    pub wiki_auto_enabled: bool,
    /// How often to poll the wiki repo for changes
    pub wiki_period: usize,
    /// Include the README.md in the wiki
    pub wiki_readme: bool,

    /// Path to the Site Git Repository
    pub site_repo: Option<String>,
    /// Link on the...
    pub site_link: LinkLocation,
    /// Do we run the site update & polling system
    pub site_auto_enabled: bool,
    /// How often to poll the site repo for changes
    pub site_period: usize,
    /// Which file to embed in your index
    pub site_embed_index: String,
    /// Include the README.md in the site
    pub site_readme: bool,

    /// User Pages
    pub users_pages_enabled: bool,
    /// User Profile Fields
    pub user_profile_field: String,
    /// User Period
    pub user_period: usize,
    /// User README
    pub user_readme: bool,
}

impl Default for PagesConfig {
    fn default() -> Self {
        Self {
            wiki_repo: Some("https://github.com/neiam/css-wiki-example".to_string()),
            wiki_auto_enabled: false,
            wiki_link: LinkLocation::Both,
            wiki_period: 600,
            wiki_readme: false,
            site_repo: Some("https://github.com/neiam/css-site-example".to_string()),
            site_auto_enabled: false,
            site_link: LinkLocation::Both,
            site_period: 600,
            site_readme: false,
            site_embed_index: "INDEX.md".to_string(),
            users_pages_enabled: false,
            user_profile_field: "user_page_repository".to_string(),
            user_period: 900,
            user_readme: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeConfig {
    pub edge_enabled: bool,
    pub edge_mqtt_config: Option<MqttConfig>,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            edge_enabled: false,
            edge_mqtt_config: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCategoryMapping {
    /// The internal enum value (e.g., "saw", "powertool", "hand_tools")
    pub value: String,
    /// The display label (e.g., "Saw", "Power Tools", "Hand Tools")
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// List of tool categories with their internal values and display labels
    pub tool_categories: Vec<ToolCategoryMapping>,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            tool_categories: vec![
                ToolCategoryMapping {
                    value: "saw".to_string(),
                    label: "Saw".to_string(),
                },
                ToolCategoryMapping {
                    value: "powertool".to_string(),
                    label: "Power Tools".to_string(),
                },
                ToolCategoryMapping {
                    value: "hand_tools".to_string(),
                    label: "Hand Tools".to_string(),
                },
                ToolCategoryMapping {
                    value: "measuring".to_string(),
                    label: "Measuring".to_string(),
                },
                ToolCategoryMapping {
                    value: "safety".to_string(),
                    label: "Safety".to_string(),
                },
                ToolCategoryMapping {
                    value: "electronics".to_string(),
                    label: "Electronics".to_string(),
                },
                ToolCategoryMapping {
                    value: "woodworking".to_string(),
                    label: "Woodworking".to_string(),
                },
                ToolCategoryMapping {
                    value: "metalworking".to_string(),
                    label: "Metalworking".to_string(),
                },
                ToolCategoryMapping {
                    value: "3d_printing".to_string(),
                    label: "3D Printing".to_string(),
                },
                ToolCategoryMapping {
                    value: "laser_cutting".to_string(),
                    label: "Laser Cutting".to_string(),
                },
                ToolCategoryMapping {
                    value: "welding".to_string(),
                    label: "Welding".to_string(),
                },
                ToolCategoryMapping {
                    value: "other".to_string(),
                    label: "Other".to_string(),
                },
            ],
        }
    }
}

/// cmi5 (xAPI) training-module settings.
///
/// Disabled by default: enabling it stands up the launch/LRS surface and the
/// filesystem content store, which an instance that does not use cmi5 has no
/// reason to expose. `content_dir` is where imported packages are extracted and
/// served from; `max_package_bytes` caps an upload; the TTLs bound the launch
/// handshake (a short-lived one-time fetch token, a longer session credential).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cmi5Config {
    /// Whether the cmi5 subsystem is active.
    pub enabled: bool,
    /// Directory imported packages are extracted to and served from.
    pub content_dir: String,
    /// Maximum accepted size of an uploaded package, in bytes.
    pub max_package_bytes: usize,
    /// Lifetime of the one-time launch `fetch` token, in seconds.
    pub fetch_ttl_secs: u64,
    /// Lifetime of an issued session credential, in seconds.
    pub session_ttl_secs: u64,
}

impl Default for Cmi5Config {
    fn default() -> Self {
        Self {
            enabled: false,
            content_dir: "./cmi5-content".to_string(),
            max_package_bytes: 100 * 1024 * 1024,
            fetch_ttl_secs: 300,
            session_ttl_secs: 4 * 60 * 60,
        }
    }
}

/// Groups.io mailing-list integration module configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupsioConfig {
    /// Master toggle. When false, all Groups.io endpoints reject, no sync
    /// runs, and no membership is pushed or pulled.
    pub enabled: bool,
    /// Groups.io API key, sent as a bearer token. Secret; never exposed to the
    /// SPA.
    pub api_key: String,
    /// Base URL of the Groups.io API.
    pub base_url: String,
    /// Identifier of the target group whose roster we mirror. The exact form
    /// (numeric id vs subdomain/name) is settled against the live account.
    pub group_id: String,
    /// How often the reconciliation loop diffs our intended roster against the
    /// group, in seconds.
    pub sync_interval_secs: u64,
    /// Shared secret used to verify inbound Groups.io membership webhooks.
    /// Secret; never exposed to the SPA. Empty means no inbound webhook is
    /// trusted (the reconciliation poll still catches every opt-out).
    pub webhook_secret: String,
    /// Addresses reconciliation must never remove even though the platform did
    /// not add them -- the group owner, moderators, and any service accounts.
    /// Because the platform owns the whole list, without this allowlist the
    /// first sync would remove the group's own managers.
    pub protected_addresses: Vec<String>,
}

impl Default for GroupsioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            base_url: "https://groups.io/api/v1".to_string(),
            group_id: String::new(),
            sync_interval_secs: 300,
            webhook_secret: String::new(),
            protected_addresses: Vec::new(),
        }
    }
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Site-specific settings
    pub site: SiteConfig,
    /// Email configuration
    pub email: EmailConfig,
    /// Theme and UI settings
    pub theme: ThemeConfig,
    /// Stripe payment settings
    pub stripe: StripeConfig,
    /// Reporting configuration
    pub reports: ReportConfig,
    /// Space directory settings
    pub space_directory: SpaceDirectoryConfig,
    /// Sentry error tracking
    pub sentry: SentryConfig,
    /// Database configuration
    pub database: DatabaseConfig,
    /// Server configuration
    pub server: ServerConfig,
    /// Authentication configuration
    pub auth: AuthConfig,
    /// Registration challenge configuration
    pub registration_challenge: RegistrationChallengeConfig,
    /// Initial setup configuration
    pub initial_setup: InitialSetupConfig,
    /// User profile configuration
    pub user: UserConfig,
    /// Tool Configs
    pub tools: ToolConfig,
    /// ToolGuard Configuration
    pub toolguard: ToolGuardConfig,
    /// Calendar configuration
    pub calendar: CalendarConfig,
    /// Pages Configuration
    pub pages: PagesConfig,
    /// Edge Config
    pub edge: EdgeConfig,
    /// Door access module configuration
    #[serde(default)]
    pub door: DoorConfig,
    /// Configurable hierarchy of places
    #[serde(default)]
    pub place: PlaceConfig,
    /// cmi5 training-module configuration
    #[serde(default)]
    pub cmi5: Cmi5Config,
    /// Groups.io mailing-list integration
    #[serde(default)]
    pub groupsio: GroupsioConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            site: SiteConfig::default(),
            email: EmailConfig::default(),
            theme: ThemeConfig::default(),
            stripe: StripeConfig::default(),
            reports: ReportConfig::default(),
            space_directory: SpaceDirectoryConfig::default(),
            sentry: SentryConfig::default(),
            database: DatabaseConfig::default(),
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            registration_challenge: RegistrationChallengeConfig::default(),
            initial_setup: InitialSetupConfig::default(),
            user: UserConfig::default(),
            tools: ToolConfig::default(),
            toolguard: ToolGuardConfig::default(),
            calendar: CalendarConfig::default(),
            pages: PagesConfig::default(),
            edge: EdgeConfig::default(),
            door: DoorConfig::default(),
            place: PlaceConfig::default(),
            cmi5: Cmi5Config::default(),
            groupsio: GroupsioConfig::default(),
        }
    }
}

/// Returned when `from_file` filled in fields a configuration was missing and
/// rewrote it. The operator has to review the result before the server runs.
///
/// This is an error type rather than a `std::process::exit` inside the loader,
/// and the distinction is the whole point of it existing.
///
/// `from_file` used to call `std::process::exit(0)` here. Zero is what a
/// program returns when it did what it was asked, so systemd saw a clean stop
/// and did not restart, `docker run` reported success, and any orchestrator
/// watching exit codes was told the server had finished normally — while the
/// server had in fact refused to start and rewritten its own configuration on
/// the way out. A deployment could sit down for the length of a config upgrade
/// and every automated signal would say it was fine.
///
/// The second reason is testability, and it is not a minor one: a test calling
/// `from_file` would have terminated the whole test binary reporting success,
/// taking every test scheduled after it with it, silently.
#[derive(Debug)]
pub struct ConfigRewritten {
    /// The configuration file, now containing defaults for what was missing.
    pub path: PathBuf,
    /// The copy of the file as it was before the rewrite.
    pub backup: PathBuf,
}

impl fmt::Display for ConfigRewritten {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the configuration at {} was missing required fields; defaults were \
             written in and the original was backed up to {}. Review the result \
             and start the server again.",
            self.path.display(),
            self.backup.display()
        )
    }
}

impl std::error::Error for ConfigRewritten {}

impl AppConfig {
    /// Load configuration from a TOML file at boot. If the file is missing
    /// fields added since it was written, self-heals by merging in
    /// defaults, backing up the old file, and exiting the process so the
    /// operator restarts against the corrected file — safe here because
    /// nothing is serving traffic yet.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_file_impl(path, true)
    }

    /// Load configuration from a TOML file for a live reload. Never exits
    /// the process: an admin editing config.toml on a running server and
    /// triggering a reload with a mistake in it (e.g. a section copied in
    /// without every required field) must get an error back, not have the
    /// self-heal-and-restart behavior above take down the whole server for
    /// every connected user.
    pub fn from_file_for_reload<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_file_impl(path, false)
    }

    fn from_file_impl<P: AsRef<Path>>(path: P, allow_recovery: bool) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

        // Try to parse the configuration
        let config_result: Result<AppConfig, toml::de::Error> = toml::from_str(&content);

        let mut config = match config_result {
            Ok(cfg) => cfg,
            Err(e) => {
                // Check if error is about missing fields
                let error_msg = e.to_string();
                if error_msg.contains("missing field") && allow_recovery {
                    eprintln!("\n⚠️  Configuration file is missing required fields!");
                    eprintln!("Error: {}\n", e);

                    // Create a backup of the old config with unix timestamp
                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let backup_path = format!("{}.{}.backup", path.as_ref().display(), timestamp);
                    fs::copy(&path, &backup_path)
                        .with_context(|| format!("Failed to create backup at: {}", backup_path))?;
                    eprintln!("📦 Backed up old config to: {}\n", backup_path);

                    // Generate updated config with defaults for missing fields
                    let default_config = AppConfig::default();

                    // Try to parse as a partial config to preserve existing values
                    // We'll use toml::Value to merge configs
                    let existing_value: toml::Value = toml::from_str(&content)
                        .unwrap_or(toml::Value::Table(toml::map::Map::new()));
                    let default_value: toml::Value = match toml::to_string(&default_config) {
                        Ok(s) => {
                            toml::from_str(&s).unwrap_or(toml::Value::Table(toml::map::Map::new()))
                        }
                        Err(_) => toml::Value::Table(toml::map::Map::new()),
                    };

                    // Merge: existing values take precedence, defaults fill in missing fields
                    let merged_value = merge_toml_values(existing_value, default_value);

                    // Parse the merged config
                    let merged_text = toml::to_string(&merged_value)
                        .with_context(|| "Failed to serialize the merged configuration")?;
                    let merged_config: AppConfig = toml::from_str(&merged_text)
                        .with_context(|| "Failed to parse merged configuration")?;

                    // Write the updated config back to file
                    merged_config
                        .to_file(&path)
                        .with_context(|| "Failed to write updated configuration")?;

                    return Err(anyhow::Error::new(ConfigRewritten {
                        path: path.as_ref().to_path_buf(),
                        backup: PathBuf::from(&backup_path),
                    }));
                } else {
                    // Some other parsing error
                    return Err(e).with_context(|| "Failed to parse TOML configuration");
                }
            }
        };

        // Apply environment variable overrides
        config.apply_env_overrides()?;

        Ok(config)
    }

    /// Apply environment variable overrides to configuration
    /// Supports nested configuration using double underscore separator
    /// Example: DATABASE__URL, DATABASE__HOST, AUTH__JWT_SECRET
    fn apply_env_overrides(&mut self) -> Result<()> {
        use std::env;

        // Database overrides
        if let Ok(val) = env::var("DATABASE__URL") {
            info!("Overriding database.url from environment");
            self.database.url = Some(val);
        }

        if let Ok(val) = env::var("SERVER__BIND_ADDRESS") {
            info!("Overriding server.bind_address from environment");
            self.server.bind_address = val;
        }

        Ok(())
    }

    /// Save configuration to a TOML file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize configuration to TOML")?;

        // Create parent directories if they don't exist
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file: {}", path.as_ref().display()))?;

        Ok(())
    }

    /// Check if initial setup is enabled and this email should get admin role
    pub fn should_grant_admin_role(&self, email: &str) -> bool {
        self.initial_setup.setup_enabled
            && email.to_lowercase() == self.initial_setup.setup_admin_email.to_lowercase()
    }
}

/// Configuration manager for runtime reloading
pub struct ConfigManager {
    config: Arc<RwLock<AppConfig>>,
    config_path: Option<std::path::PathBuf>,
}

impl ConfigManager {
    /// Create a new configuration manager
    pub fn new(config: AppConfig, config_path: Option<std::path::PathBuf>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        }
    }

    /// Get the current configuration (read-only)
    pub fn get_config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    /// Reload configuration from disk
    pub fn reload_config(&self) -> Result<()> {
        let config_path = self
            .config_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No config path available for reloading"))?;

        info!("Reloading configuration from: {}", config_path.display());

        let new_config = AppConfig::from_file_for_reload(config_path)
            .with_context(|| "Failed to reload configuration")?;

        // Validate the new configuration
        validate_config(&new_config)?;

        // Update the configuration atomically
        {
            let mut config_guard = self.config.write().unwrap();
            *config_guard = new_config;
        }

        info!("Configuration reloaded successfully");
        Ok(())
    }

    /// Get a thread-safe reference to the configuration
    pub fn get_config_ref(&self) -> Arc<RwLock<AppConfig>> {
        Arc::clone(&self.config)
    }

    /// Overwrite the in-memory profile field schema without touching the
    /// config file. The version-history table in the database is the
    /// source of truth; this only keeps the seed slot in sync as a
    /// fallback for the (post-bootstrap, shouldn't normally happen) case
    /// where nothing has read a DB version yet.
    pub fn set_profile_fields(&self, profile_fields: Vec<ProfileField>) {
        self.config.write().unwrap().user.profile_fields_seed = profile_fields;
    }

    /// Apply an in-place mutation to the shared configuration and persist
    /// the result to the config file, if one is known.
    pub fn update_config<F>(&self, mutator: F) -> Result<()>
    where
        F: FnOnce(&mut AppConfig),
    {
        let updated = {
            let mut config_guard = self.config.write().unwrap();
            mutator(&mut config_guard);
            config_guard.clone()
        };

        if let Some(config_path) = &self.config_path {
            updated
                .to_file(config_path)
                .with_context(|| "Failed to persist updated configuration")?;
        } else {
            warn!("No config path available; configuration change was not persisted to disk");
        }

        Ok(())
    }

    /// Overwrite the in-memory `profiles_enabled` seed without touching
    /// the config file. Same caveat as `set_profile_fields`: the
    /// `profile_config_versions` table is authoritative, this is just the
    /// fallback slot.
    pub fn set_profiles_enabled(&self, enabled: bool) {
        self.config.write().unwrap().user.profiles_enabled_seed = enabled;
    }
}

/// Validate configuration before applying it, whether at startup or reload.
fn validate_config(config: &AppConfig) -> Result<()> {
    // Reject the well-known placeholder JWT secret outright: it's public
    // (it's the code default, and has shipped in tracked config files), so
    // a server signing tokens with it is a full auth bypass, not just an
    // insecure setting to warn about.
    if config.auth.jwt_secret == LEGACY_DEFAULT_JWT_SECRET {
        return Err(anyhow::anyhow!(
            "auth.jwt_secret is still set to the default placeholder value. \
             Set a real, random secret before starting (or delete the [auth] \
             section and restart to have one generated automatically)."
        ));
    }

    // An empty secret is worse than the placeholder: HMAC accepts a zero-length
    // key, so the server would sign and verify tokens with a key an attacker
    // does not even have to look up. `load_config` fills an absent secret in
    // before it gets here, so reaching this means either a config that set it
    // to "" explicitly, or a live reload of one -- and the reload path is why
    // this lives in `validate_config` rather than next to the generation.
    if config.auth.jwt_secret.is_empty() {
        return Err(anyhow::anyhow!(
            "auth.jwt_secret is empty. Remove the key entirely to have a \
             secret generated and written back, or set a real random value; \
             an empty secret signs tokens with a zero-length HMAC key."
        ));
    }

    // Validate database connection string
    if config.database.get_url().is_empty() {
        return Err(anyhow::anyhow!("Database URL cannot be empty"));
    }

    // Validate server bind address
    if config.server.bind_address.is_empty() {
        return Err(anyhow::anyhow!("Server bind address cannot be empty"));
    }

    // A mailer that is switched on and cannot possibly send is worse than one
    // that is switched off: the first reports "check your email" to a member
    // who will never receive one. Refused at load and at reload, so an operator
    // who breaks it while editing finds out immediately rather than at the
    // first password reset.
    //
    // An empty `password` is deliberately NOT refused -- submission relays that
    // permit unauthenticated senders are legitimate, and `MailService` only
    // offers AUTH when a username is set.
    if config.email.enabled {
        if config.email.host.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "email.enabled is true but email.host is empty, so no message \
                 could ever be delivered. Set a host, or set email.enabled = false."
            ));
        }
        if !config.email.from_email.contains('@') {
            return Err(anyhow::anyhow!(
                "email.enabled is true but email.from_email ({:?}) is not an \
                 email address. Every message would be rejected by the relay.",
                config.email.from_email
            ));
        }
    }

    // The lockout guard.
    //
    // require_email_verification with no mailer means nobody can confirm an
    // address, so nobody who registers after the flag is set can ever log in,
    // and the operator has no way to tell that is why. Refused at boot and at
    // reload rather than warned about: a server that will not start with a
    // message naming both settings is a far better outcome than one that
    // starts and quietly refuses everyone.
    if config.auth.require_email_verification && !config.email.enabled {
        return Err(anyhow::anyhow!(
            "auth.require_email_verification is true but email.enabled is false. \
             Nobody could confirm an address, so no account created after this \
             point could ever sign in. Configure [email] and enable it, or turn \
             require_email_verification off."
        ));
    }

    // A Groups.io sync switched on but unable to authenticate, or not told
    // which group to mirror, can only fail every cycle -- and because the
    // platform owns the whole list, a group id left blank (or pointed at the
    // wrong group) would try to reshape a roster that is not ours. Refused at
    // boot and at reload, like the email guard above, so a misconfiguration is
    // caught before the first destructive reconciliation rather than after it.
    if config.groupsio.enabled {
        if config.groupsio.api_key.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "groupsio.enabled is true but groupsio.api_key is empty, so no \
                 request to Groups.io could authenticate. Set an API key, or set \
                 groupsio.enabled = false."
            ));
        }
        if config.groupsio.group_id.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "groupsio.enabled is true but groupsio.group_id is empty, so the \
                 sync would not know which group to mirror. Set the target group, \
                 or set groupsio.enabled = false."
            ));
        }
    }

    // CORS switched on with nothing to allow. The same failure the email guard
    // above refuses, in a different subsystem: a capability turned on that
    // cannot do anything except mislead. Refused here rather than defaulted to
    // a wildcard, because '*' on a bearer-token API is a data exposure, not a
    // permissive convenience. See `cors::build_layer`.
    crate::cors::validate(&config.server)?;

    // Validate initial setup admin email format
    if config.initial_setup.setup_enabled && !config.initial_setup.setup_admin_email.contains('@') {
        return Err(anyhow::anyhow!(
            "Initial setup admin email must be a valid email address"
        ));
    }

    Ok(())
}

/// Load configuration from file or create default configuration
pub fn load_config<P: AsRef<Path>>(config_path: P) -> Result<AppConfig> {
    let path = config_path.as_ref();

    let mut config = if path.exists() {
        println!("Loading configuration from: {}", path.display());
        AppConfig::from_file(path)?
    } else {
        println!(
            "Config file not found. Creating default configuration at: {}",
            path.display()
        );
        let default_config = AppConfig::default();

        // Save default configuration to file
        default_config
            .to_file(path)
            .with_context(|| "Failed to create default configuration file")?;

        println!("Default configuration file created. Please review and modify as needed.");
        default_config
    };

    // An existing file that omits auth.jwt_secret gets one generated here and
    // written straight back. Persisting is the whole point: an in-memory-only
    // secret would be different on every boot, so every restart would silently
    // sign out every user, and the config file would still look fine.
    //
    // The file-did-not-exist branch above already persisted, via `to_file` on a
    // fresh `AppConfig::default()`; this covers the branch that read a file.
    if config.auth.jwt_secret.is_empty() {
        println!("auth.jwt_secret is not set; generating one and writing it to the config file.");
        config.auth.jwt_secret = generate_default_jwt_secret();
        config
            .to_file(path)
            .with_context(|| "Failed to persist the generated auth.jwt_secret")?;
    }

    validate_config(&config)?;

    Ok(config)
}

/// Generate a sample configuration file with comments
pub fn generate_sample_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let default_config = AppConfig::default();

    default_config
        .to_file(&path)
        .with_context(|| "Failed to write sample configuration file")?;

    println!(
        "Sample configuration file generated at: {}",
        path.as_ref().display()
    );
    println!("Please review and modify the configuration as needed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config_serialization() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();

        // Verify we can deserialize it back
        let _: AppConfig = toml::from_str(&toml_str).unwrap();
    }

    #[test]
    fn test_config_file_roundtrip() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = AppConfig::default();

        // Save to file
        config.to_file(temp_file.path()).unwrap();

        // Load from file
        let loaded_config = AppConfig::from_file(temp_file.path()).unwrap();

        // Compare (using debug format since we don't implement PartialEq)
        assert_eq!(format!("{:?}", config), format!("{:?}", loaded_config));
    }

    // -----------------------------------------------------------------------
    // The live reload path
    // -----------------------------------------------------------------------
    // `ConfigManager::reload_config` is wired to `POST /api/admin/reload-config`
    // and runs against a server that is already serving traffic. That is a
    // different situation from boot and it needs a different loader.
    //
    // `from_file` responds to a missing field by merging in defaults, backing
    // the old file up, and rewriting the original in place. At boot that is a
    // service: nothing is serving yet, and the alternative is a deployment that
    // will not start. On a live reload it is a surprise -- the admin edited the
    // file by hand thirty seconds ago, and the endpoint answers "failed to
    // reload" while their edit has been replaced by a wall of defaults and
    // survives only in a timestamped backup they have no reason to look for.
    //
    // `from_file_for_reload` exists for exactly this and says so in its own doc
    // comment. It was introduced *and wired up* in one commit; a later merge
    // kept the function and dropped the call site, leaving a reload-safety
    // helper with no callers anywhere in the tree. See issue #9.

    /// A valid configuration on disk, and a manager pointed at it.
    fn manager_on_disk(dir: &std::path::Path) -> (ConfigManager, std::path::PathBuf) {
        let path = dir.join("config.toml");
        let config = AppConfig::default();
        config.to_file(&path).expect("write the fixture config");
        let manager = ConfigManager::new(config, Some(path.clone()));
        (manager, path)
    }

    /// Every `*.backup` sibling, which is what the boot loader's recovery path
    /// leaves behind and the reload path must not.
    fn backups_beside(path: &std::path::Path) -> Vec<std::path::PathBuf> {
        let dir = path.parent().expect("the config has a parent directory");
        std::fs::read_dir(dir)
            .expect("read the config directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| p.to_string_lossy().contains(".backup"))
            .collect()
    }

    /// Remove a required field from a serialized configuration.
    ///
    /// The assertion that the removal changed something is not decoration; it
    /// is the same guard the boot-loader tests above carry. A fixture that
    /// removes a line the serializer never emits parses cleanly, and the test
    /// then passes for the wrong reason while looking like it proves something.
    fn without_a_required_field(text: &str) -> String {
        let broken: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("lookahead_days"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(
            text.lines().count(),
            broken.lines().count(),
            "the fixture removed nothing; AppConfig no longer serializes lookahead_days"
        );
        broken
    }

    /// **The regression test for issue #9.**
    ///
    /// Mutation check: change `from_file_for_reload` back to `from_file` in
    /// `reload_config` and this fails on the file-contents assertion, naming
    /// the rewrite.
    #[test]
    fn a_failed_reload_does_not_rewrite_the_operators_file() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (manager, path) = manager_on_disk(dir.path());

        // Anti-vacuity: the untouched fixture reloads cleanly, so a failure
        // below is attributable to the field this test removes rather than to a
        // fixture that never validated in the first place.
        manager
            .reload_config()
            .expect("the untouched fixture must reload");

        let broken =
            without_a_required_field(&std::fs::read_to_string(&path).expect("read the fixture"));
        std::fs::write(&path, &broken).expect("write the broken config");

        let err = manager
            .reload_config()
            .expect_err("a reload of a config missing a required field must fail");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            broken,
            "the reload rewrote the operator's config file in place"
        );
        assert!(
            backups_beside(&path).is_empty(),
            "the reload took the boot-time recovery path and left {:?}",
            backups_beside(&path)
        );
        assert!(
            err.downcast_ref::<ConfigRewritten>().is_none(),
            "a live reload returned ConfigRewritten, which only the boot loader \
             should ever produce"
        );
    }

    /// A failed reload leaves the server running on what it already had.
    ///
    /// The in-memory configuration is still valid -- it is what the process
    /// booted with -- so a bad edit must not be able to degrade a running
    /// server, only to be rejected.
    #[test]
    fn a_failed_reload_leaves_the_running_configuration_in_place() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (manager, path) = manager_on_disk(dir.path());

        let before = manager.get_config();
        let broken =
            without_a_required_field(&std::fs::read_to_string(&path).expect("read the fixture"));
        std::fs::write(&path, &broken).expect("write the broken config");

        manager
            .reload_config()
            .expect_err("a config missing a required field must not load");

        assert_eq!(
            format!("{:?}", manager.get_config()),
            format!("{:?}", before),
            "a rejected reload changed the running configuration"
        );
    }

    /// A file that is not TOML at all is rejected without a rewrite.
    ///
    /// Distinct from the missing-field case: that one is a config a version
    /// upgrade left incomplete, and merging defaults into it is defensible at
    /// boot. This one is a typo, and rewriting it would replace whatever the
    /// operator meant to write.
    #[test]
    fn a_reload_of_invalid_toml_does_not_rewrite_it() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (manager, path) = manager_on_disk(dir.path());

        manager
            .reload_config()
            .expect("the untouched fixture must reload");

        std::fs::write(&path, "this is not toml = = =").expect("write");

        manager
            .reload_config()
            .expect_err("invalid TOML must not load");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "this is not toml = = =",
            "the reload modified a file it could not parse"
        );
        assert!(
            backups_beside(&path).is_empty(),
            "invalid TOML was treated as a missing-field migration"
        );
    }

    /// The other half: a valid edit has to actually take effect.
    ///
    /// Without this, `reload_config` could satisfy every assertion above by
    /// never reading the file at all.
    #[test]
    fn a_successful_reload_replaces_the_running_configuration() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let (manager, path) = manager_on_disk(dir.path());

        assert_ne!(
            manager.get_config().site.site_name,
            "Renamed By Reload",
            "the fixture already carries the value this test writes"
        );

        let mut edited = manager.get_config();
        edited.site.site_name = "Renamed By Reload".to_string();
        edited.to_file(&path).expect("write the edited config");

        manager
            .reload_config()
            .expect("a valid edit must reload cleanly");

        assert_eq!(
            manager.get_config().site.site_name,
            "Renamed By Reload",
            "the reload reported success without picking up the edit"
        );
        assert!(
            backups_beside(&path).is_empty(),
            "a successful reload should not back anything up"
        );
    }

    /// The structural companion, and the one that survives a merge.
    ///
    /// Issue #9 was not a mistake in either loader -- both behave correctly for
    /// what they were written to do. It was a merge that kept one branch's
    /// function split alongside the other branch's call site, and no warning
    /// fires for a `pub fn` with no callers in a library crate. The behavioral
    /// tests above catch the defect; this one catches the shape of it, in the
    /// place a future merge would reintroduce it.
    #[test]
    fn the_reload_path_calls_the_reload_loader() {
        const SOURCE: &str = include_str!("config.rs");

        let after = SOURCE
            .split_once("pub fn reload_config")
            .expect("config.rs no longer defines reload_config; rewrite this check")
            .1;
        let body = after
            .split_once("\n    }\n")
            .expect("reload_config's body is not delimited as expected; rewrite this check")
            .0;

        // Anti-vacuity: a body located incorrectly would be empty, and an empty
        // body satisfies the negative assertion below while proving nothing.
        assert!(
            body.contains("config_path"),
            "reload_config's body was not located correctly; this check needs \
             rewriting rather than deleting"
        );
        assert!(
            body.contains("from_file_for_reload("),
            "reload_config does not use the reload loader -- see issue #9"
        );
        assert!(
            !body.contains("from_file("),
            "reload_config calls the boot loader `from_file`, which rewrites a \
             config missing a field in place. Correct at boot, wrong for a live \
             reload against a server that is serving traffic. See issue #9."
        );
    }

    /// A configuration missing a required field must come back as an error.
    ///
    /// **This test could not have existed before the change it covers.**
    /// `from_file` responded to a missing field by calling
    /// `std::process::exit(0)`, so running this would have terminated the whole
    /// test binary — reporting success — and silently taken every test
    /// scheduled after it. That is not a hypothetical: the stack battery's
    /// first successful bring-up found the same code path in production, where
    /// it told the container runtime the server had finished normally after
    /// refusing to start.
    #[test]
    fn requiring_confirmed_addresses_without_a_mailer_is_refused() {
        // The lockout guard, and the reason it is an error rather than a
        // warning. With no mailer nobody can confirm an address, so nobody who
        // registers after the flag is set could ever sign in -- and the
        // operator would have nothing to tell them why. Refusing to start names
        // both settings.
        //
        // Mutation check: change the condition in `validate_config` to `false`
        // and this fails.
        let mut config = AppConfig::default();
        config.auth.require_email_verification = true;
        config.email.enabled = false;

        let err = validate_config(&config)
            .expect_err("requiring confirmation with no way to confirm is unusable");
        let text = err.to_string();
        assert!(
            text.contains("require_email_verification") && text.contains("email.enabled"),
            "the refusal must name both settings, since either one is a valid \
             thing to change: {text}"
        );
    }

    #[test]
    fn requiring_confirmed_addresses_with_a_mailer_is_fine() {
        // Anti-vacuity for the test above: if `validate_config` started
        // refusing `require_email_verification` outright, that test would still
        // pass while the feature had become unusable.
        let mut config = AppConfig::default();
        config.auth.require_email_verification = true;
        config.email.enabled = true;
        config.email.host = "smtp.example.invalid".to_string();
        config.email.from_email = "noreply@example.invalid".to_string();

        validate_config(&config)
            .expect("a configured mailer is exactly what makes the flag usable");
    }

    #[test]
    fn a_disabled_mailer_is_never_refused_however_empty_it_is() {
        // Anti-vacuity for the two tests below. The shipped default has
        // `enabled = false` with a `localhost` host and an example.com sender;
        // if validation ever started rejecting that, every existing deployment
        // would fail to boot on upgrade.
        let mut config = AppConfig::default();
        config.email.enabled = false;
        config.email.host = String::new();
        config.email.from_email = String::new();

        validate_config(&config).expect("a disabled mailer imposes no requirements");
    }

    #[test]
    fn an_enabled_mailer_with_no_host_is_refused() {
        // Mutation check: delete the `email.host` arm in `validate_config` and
        // this fails. The defect it guards is a deployment that reports "check
        // your email" to a member who will never receive one.
        let mut config = AppConfig::default();
        config.email.enabled = true;
        config.email.host = "   ".to_string();

        let err = validate_config(&config)
            .expect_err("an enabled mailer with no host cannot deliver anything");
        assert!(
            err.to_string().contains("email.host"),
            "the refusal should name the field an operator has to fix, got: {err}"
        );
    }

    #[test]
    fn an_enabled_mailer_with_a_nonsense_sender_is_refused() {
        let mut config = AppConfig::default();
        config.email.enabled = true;
        config.email.from_email = "not an address".to_string();

        let err = validate_config(&config)
            .expect_err("a sender that is not an address is refused by every relay");
        assert!(
            err.to_string().contains("from_email"),
            "the refusal should name the field an operator has to fix, got: {err}"
        );
    }

    #[test]
    fn a_config_missing_a_field_is_an_error_and_not_an_exit() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("config.toml");

        // A real configuration with one field removed. `lookahead_days` has no
        // `#[serde(default)]`, so omitting it is a missing-field error -- the
        // same shape that stopped the stack battery booting on its first run.
        //
        // The assertion that the removal changed something is not decoration.
        // The first version of this test removed a line the serializer does not
        // emit, so the config parsed cleanly, `from_file` succeeded, and the
        // test failed with a wall of Debug output rather than saying the
        // fixture was wrong. A mutation that does not mutate is a test that
        // proves nothing while looking like it proves something.
        let text = toml::to_string_pretty(&AppConfig::default()).expect("serialize");
        let broken: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("lookahead_days"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(
            text.lines().count(),
            broken.lines().count(),
            "the fixture removed nothing; AppConfig no longer serializes lookahead_days"
        );
        std::fs::write(&path, &broken).expect("write the config");

        let err = AppConfig::from_file(&path)
            .expect_err("a config missing a required field must not load silently");

        let rewritten = err
            .downcast_ref::<ConfigRewritten>()
            .expect("the caller has to be able to tell this apart from a parse failure");

        assert_eq!(rewritten.path, path, "the error names the file it rewrote");
        assert!(
            rewritten.backup.exists(),
            "the original was not backed up to {}, so the operator's file is gone",
            rewritten.backup.display()
        );

        // And the rewrite happened: the file now loads.
        AppConfig::from_file(&path)
            .expect("the rewritten configuration should load on the second attempt");
    }

    /// A file that is not TOML at all is a plain parse failure, not a rewrite.
    ///
    /// The two are different: one is a configuration a version upgrade left
    /// incomplete, and rewriting it is a service. The other is a typo, and
    /// rewriting *that* would replace whatever the operator meant to write with
    /// a wall of defaults.
    #[test]
    fn a_config_that_is_not_toml_is_not_rewritten() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is not toml = = =").expect("write");

        let err = AppConfig::from_file(&path).expect_err("invalid TOML must not load");
        assert!(
            err.downcast_ref::<ConfigRewritten>().is_none(),
            "invalid TOML was treated as a missing-field migration and the file was rewritten"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "this is not toml = = =",
            "the file was modified"
        );
    }

    /// The shipped sample must be loadable.
    ///
    /// It is the first thing a new deployment copies, and a sample that does
    /// not parse turns the very first boot into the rewrite path above — which
    /// then hands the operator `PagesConfig::default()`'s live GitHub URLs and
    /// starts cloning them.
    ///
    /// Asserted with `toml::from_str` rather than `from_file`, deliberately:
    /// `from_file` would repair the file in place, so a broken sample would
    /// make this test pass and quietly edit a tracked file in the working tree.
    #[test]
    fn the_shipped_sample_config_parses() {
        let sample = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.sample.toml");
        let text = std::fs::read_to_string(sample)
            .unwrap_or_else(|e| panic!("config.sample.toml must exist and be readable: {e}"));
        let config: AppConfig = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("config.sample.toml does not parse as an AppConfig: {e}"));

        // And it must not ship pointing at somebody's repositories. PagesService
        // git-clones these at boot into a hardcoded /tmp path.
        assert!(
            config.pages.wiki_repo.is_none() && config.pages.site_repo.is_none(),
            "the sample names a wiki or site repository; a fresh deployment would \
             clone it on first boot without anybody asking for it"
        );
    }

    /// The sample deliberately ships no `auth.jwt_secret`, because a real one
    /// written into a tracked sample is a public secret the moment anybody
    /// copies the file. That only works if absence is a valid parse.
    ///
    /// Pinned separately from the parse test above so the reason survives: if
    /// somebody "fixes" a future parse failure by putting a value back into the
    /// sample, this fails and says why.
    #[test]
    fn the_sample_ships_no_jwt_secret() {
        let sample = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.sample.toml");
        let text = std::fs::read_to_string(sample).expect("config.sample.toml");

        let raw: toml::Value = toml::from_str(&text).expect("sample is valid TOML");
        let present = raw.get("auth").and_then(|a| a.get("jwt_secret")).is_some();

        assert!(
            !present,
            "config.sample.toml sets auth.jwt_secret. Whatever value is there \
             is now public, and every deployment that copies this file signs \
             its tokens with it. Remove the key: load_config generates one and \
             writes it back on first boot."
        );

        // And the absence really does deserialize, rather than only being
        // tolerated by a `from_file` repair that would edit a tracked file.
        let config: AppConfig = toml::from_str(&text).expect("sample parses");
        assert!(config.auth.jwt_secret.is_empty());
    }

    /// An absent secret must come back *generated and persisted*, not
    /// generated per boot. The persistence half is the one that fails
    /// silently: sessions would die on every restart and the config file
    /// would still look correct.
    #[test]
    fn an_absent_jwt_secret_is_generated_and_written_back() {
        // Absence, not `jwt_secret = ""`. The sample's shape is a missing key,
        // and a test that wrote an empty string would pass even if serde still
        // required the field.
        let temp_file = NamedTempFile::new().unwrap();
        let mut seed = AppConfig::default();
        seed.auth.jwt_secret = "placeholder-to-be-stripped".to_string();
        seed.to_file(temp_file.path()).unwrap();

        let text = std::fs::read_to_string(temp_file.path()).unwrap();
        let stripped: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("jwt_secret"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !stripped.contains("jwt_secret"),
            "the key was not actually removed, so this test proves nothing"
        );
        std::fs::write(temp_file.path(), &stripped).unwrap();

        let first = load_config(temp_file.path()).expect("load generates a secret");
        assert!(
            !first.auth.jwt_secret.is_empty(),
            "an absent secret was left empty; validate_config should have refused \
             it, and if it did not the server signs with a zero-length HMAC key"
        );
        assert_ne!(first.auth.jwt_secret, LEGACY_DEFAULT_JWT_SECRET);

        // The persistence claim, and the whole reason this is not a serde
        // default: a second load must find the same secret, not mint another.
        let second = load_config(temp_file.path()).expect("second load");
        assert_eq!(
            first.auth.jwt_secret, second.auth.jwt_secret,
            "the generated secret was not written back, so every restart mints a \
             new one and signs out every user"
        );
    }

    /// An explicitly empty secret is refused rather than used. This is the
    /// path a live `admin_config_reload` takes, which never goes through
    /// `load_config` and so is never filled in for.
    #[test]
    fn an_empty_jwt_secret_is_refused() {
        let mut config = AppConfig::default();
        config.database.url = Some("postgres://localhost/x".to_string());
        config.auth.jwt_secret = String::new();

        let err = validate_config(&config).expect_err("an empty secret must be refused");
        assert!(
            err.to_string().contains("jwt_secret"),
            "the refusal must name the field: {err}"
        );
    }

    /// A finding, recorded rather than changed.
    ///
    /// `PagesConfig::default()` names two live GitHub repositories, and
    /// `PagesService::new` git-clones whatever is there into a hardcoded
    /// /tmp/css-{wiki,site}-repo during boot. So a deployment that starts with
    /// no config file at all -- which `load_config` handles by writing the
    /// defaults and carrying on -- clones two repositories belonging to
    /// somebody else on its first boot, and fails closed the moment the
    /// network or those repositories do.
    ///
    /// Whether the shipped default should be a working demo or an empty one is
    /// a product decision, not a test's. What a test can do is make the current
    /// answer visible and stop it changing silently in either direction, which
    /// is what this is. The tracked sample has both commented out, asserted by
    /// `the_shipped_sample_config_parses` above, so the path a real operator
    /// takes is already clean; this covers the path taken by a first boot with
    /// no file.
    #[test]
    fn the_default_config_still_names_two_live_repositories() {
        let defaults = AppConfig::default();
        assert_eq!(
            defaults.pages.wiki_repo.as_deref(),
            Some("https://github.com/neiam/css-wiki-example"),
            "the default wiki repository changed. If it became None, delete this \
             test -- the finding is fixed. If it became a different URL, a first \
             boot now clones that one instead."
        );
        assert_eq!(
            defaults.pages.site_repo.as_deref(),
            Some("https://github.com/neiam/css-site-example"),
        );
        // The one thing that keeps it survivable: neither is polled, so the
        // clone happens once at boot rather than every ten minutes.
        assert!(!defaults.pages.wiki_auto_enabled);
        assert!(!defaults.pages.site_auto_enabled);
    }

    #[test]
    fn test_load_config_creates_default_if_missing() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Delete the file so it doesn't exist
        std::fs::remove_file(path).unwrap();

        // load_config should create a default config
        let _config = load_config(path).unwrap();

        // File should now exist
        assert!(path.exists());
    }

    #[test]
    fn test_load_config_rejects_legacy_default_jwt_secret() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut config = AppConfig::default();
        config.auth.jwt_secret = LEGACY_DEFAULT_JWT_SECRET.to_string();
        config.to_file(temp_file.path()).unwrap();

        let err = load_config(temp_file.path()).unwrap_err();
        assert!(err.to_string().contains("jwt_secret"));
    }
}

// ---------------------------------------------------------------------------
// Tier 1: MFA enrollment enforcement.
// ---------------------------------------------------------------------------
// `is_required_for` is the whole of the enrollment policy. It decides the
// `must_enroll_mfa` flag on the login response and the `must_enroll` field the
// settings page reads, and it is fifteen (enforcement x role) answers behind
// one `match` with a short-circuit in front of it.
//
// A partial table here would be worse than none: the enforcement variants are
// exactly the kind of thing that gets extended -- "required for staff, but not
// on the kiosk" -- and the failure mode of getting one cell wrong is either a
// role that is silently never asked to enroll, or a role locked out of a system
// it was never meant to be locked out of. So the table is exhaustive, and the
// two `match`es below are wildcard-free so that adding a variant to either enum
// fails to compile here rather than defaulting quietly to someone's guess.
#[cfg(test)]
mod mfa_enforcement_tests {
    use super::{AuthMfaConfig, MfaEnforcement};
    use crate::models::UserRole;

    const ALL_ROLES: [UserRole; 5] = [
        UserRole::Unknown,
        UserRole::Newbie,
        UserRole::Member,
        UserRole::Staff,
        UserRole::Admin,
    ];

    const ALL_ENFORCEMENTS: [MfaEnforcement; 3] = [
        MfaEnforcement::OptIn,
        MfaEnforcement::RequiredForStaff,
        MfaEnforcement::RequiredForAll,
    ];

    /// Wildcard-free on purpose: a new `UserRole` variant fails to compile
    /// here, which forces whoever adds it to decide what MFA policy it carries
    /// and to add it to `ALL_ROLES` above.
    fn role_name(role: &UserRole) -> &'static str {
        match role {
            UserRole::Unknown => "Unknown",
            UserRole::Newbie => "Newbie",
            UserRole::Member => "Member",
            UserRole::Staff => "Staff",
            UserRole::Admin => "Admin",
        }
    }

    /// Likewise for the enforcement setting.
    fn enforcement_name(e: &MfaEnforcement) -> &'static str {
        match e {
            MfaEnforcement::OptIn => "OptIn",
            MfaEnforcement::RequiredForStaff => "RequiredForStaff",
            MfaEnforcement::RequiredForAll => "RequiredForAll",
        }
    }

    fn config(enabled: bool, enforcement: MfaEnforcement) -> AuthMfaConfig {
        AuthMfaConfig {
            enabled,
            enforcement,
            ..AuthMfaConfig::default()
        }
    }

    /// The whole policy, written out rather than derived. A table computed from
    /// `is_required_for` would agree with it however it changed.
    fn expected(enforcement: MfaEnforcement, role: &UserRole) -> bool {
        match (enforcement, role_name(role)) {
            (MfaEnforcement::OptIn, _) => false,
            (MfaEnforcement::RequiredForStaff, "Staff" | "Admin") => true,
            (MfaEnforcement::RequiredForStaff, _) => false,
            (MfaEnforcement::RequiredForAll, _) => true,
        }
    }

    #[test]
    fn the_enrollment_policy_is_exactly_this_table() {
        for enforcement in ALL_ENFORCEMENTS {
            for role in &ALL_ROLES {
                let cfg = config(true, enforcement);
                assert_eq!(
                    cfg.is_required_for(role),
                    expected(enforcement, role),
                    "{} + {} answered the wrong way",
                    enforcement_name(&enforcement),
                    role_name(role),
                );
            }
        }
    }

    #[test]
    fn opt_in_never_requires_anybody() {
        // The default, and the one that must never start demanding enrollment
        // by accident: `MfaEnforcement::default()` is `OptIn`, so this is what
        // every deployment that has not thought about MFA gets.
        assert_eq!(MfaEnforcement::default(), MfaEnforcement::OptIn);
        let cfg = config(true, MfaEnforcement::OptIn);
        for role in &ALL_ROLES {
            assert!(
                !cfg.is_required_for(role),
                "OptIn required enrollment of {}",
                role_name(role)
            );
        }
    }

    #[test]
    fn required_for_staff_means_staff_and_admin_and_nobody_else() {
        let cfg = config(true, MfaEnforcement::RequiredForStaff);
        assert!(cfg.is_required_for(&UserRole::Staff));
        assert!(cfg.is_required_for(&UserRole::Admin));
        for role in [UserRole::Unknown, UserRole::Newbie, UserRole::Member] {
            assert!(
                !cfg.is_required_for(&role),
                "RequiredForStaff required enrollment of {}",
                role_name(&role)
            );
        }
    }

    #[test]
    fn required_for_all_includes_the_roles_that_are_easy_to_forget() {
        // `Unknown` is the defensive default a user gets when their role could
        // not be read, and `Newbie` is the role a self-registration lands in.
        // Both are easy to leave out of a "required for all" and neither should
        // be.
        let cfg = config(true, MfaEnforcement::RequiredForAll);
        for role in &ALL_ROLES {
            assert!(
                cfg.is_required_for(role),
                "RequiredForAll exempted {}",
                role_name(role)
            );
        }
    }

    #[test]
    fn the_master_toggle_beats_every_enforcement_setting() {
        // The short-circuit, and the reason it matters: with MFA switched off,
        // every enrollment route answers 403. A deployment that left
        // `enforcement = "RequiredForAll"` behind while turning `enabled` off
        // would otherwise tell every user they must enroll, through the only
        // door that is bolted shut. That is a total lockout of the settings
        // page, delivered by a configuration change that reads like a
        // switch-off.
        for enforcement in ALL_ENFORCEMENTS {
            let cfg = config(false, enforcement);
            for role in &ALL_ROLES {
                assert!(
                    !cfg.is_required_for(role),
                    "MFA is disabled but {} + {} still demanded enrollment",
                    enforcement_name(&enforcement),
                    role_name(role)
                );
            }
        }
    }

    #[test]
    fn the_default_config_demands_nothing_of_anybody() {
        // What a fresh install gets: disabled, opt-in. Both halves matter, and
        // a change to either is a change to what every existing deployment does
        // on its next restart.
        let cfg = AuthMfaConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.enforcement, MfaEnforcement::OptIn);
        assert!(
            cfg.allow_totp,
            "TOTP is the fallback when there is no passkey"
        );
        assert_eq!(
            cfg.recovery_code_count, 10,
            "the count is what a user is handed once and never shown again"
        );
        for role in &ALL_ROLES {
            assert!(!cfg.is_required_for(role));
        }
    }

    #[test]
    fn the_enforcement_setting_survives_a_config_round_trip() {
        // The variants are serialized into config.toml by name. A rename would
        // silently fall back to the serde default on the next load, quietly
        // downgrading a deployment from RequiredForAll to OptIn.
        for enforcement in ALL_ENFORCEMENTS {
            let cfg = config(true, enforcement);
            let text = toml::to_string(&cfg).expect("serializes");
            let back: AuthMfaConfig = toml::from_str(&text).expect("deserializes");
            assert_eq!(
                back.enforcement,
                enforcement,
                "{} did not survive a round trip through TOML",
                enforcement_name(&enforcement)
            );
        }
    }
}
