use std::fmt::{Display, Formatter};
use anyhow::{Context, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, RwLock};
use tracing::info;
pub(crate) use css_lib::{MqttConfig};

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
            terms_of_service_md: "By registering, you agree to the [terms of service](https://example.com/terms-of-service)".to_string(),
            recaptcha_enabled: false,
            recaptcha_site_key: String::new(),
            recaptcha_secret_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// JWT secret key for signing tokens
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
    Select { options: Vec<String> },
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
            global_api_key: None
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
    pub fn validate_parent_child(
        &self,
        parent_type: &str,
        child_type: &str,
    ) -> Result<(), String> {
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
            calendars: vec![
                CalendarSource {
                    ical_link: "https://example.com/calendar.ics".to_string(),
                    name: "Example Calendar".to_string(),
                    color: "#3788d8".to_string(),
                    enabled: false,
                },
            ],
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
            user_readme: true
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeConfig {
    pub edge_enabled: bool,
    pub edge_mqtt_config: Option<MqttConfig>
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
        }
    }
}

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
                    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)
                        .unwrap_or_default().as_secs();
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
                        Ok(s) => toml::from_str(&s).unwrap_or(toml::Value::Table(toml::map::Map::new())),
                        Err(_) => toml::Value::Table(toml::map::Map::new()),
                    };

                    // Merge: existing values take precedence, defaults fill in missing fields
                    let merged_value = merge_toml_values(existing_value, default_value);

                    // Parse the merged config
                    let merged_config: AppConfig = toml::from_str(&toml::to_string(&merged_value).unwrap())
                        .with_context(|| "Failed to parse merged configuration")?;

                    // Write the updated config back to file
                    merged_config.to_file(&path)
                        .with_context(|| "Failed to write updated configuration")?;

                    eprintln!("✅ Updated configuration file with default values for missing fields.");
                    eprintln!("📝 Please review the configuration at: {}", path.as_ref().display());
                    eprintln!("\n🛑 Server will now exit. Please restart after reviewing the configuration.\n");

                    std::process::exit(0);
                } else if error_msg.contains("missing field") {
                    return Err(e).with_context(|| {
                        "Config reload failed: file is missing required fields. \
                         The running server keeps its current configuration; fix \
                         the file and reload again (or restart the process, which \
                         can self-heal missing fields at boot)"
                    });
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
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
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
        let config_path = self.config_path.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No config path available for reloading"))?;

        info!("Reloading configuration from: {}", config_path.display());

        let new_config = AppConfig::from_file_for_reload(config_path)?;

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

    // Validate database connection string
    if config.database.get_url().is_empty() {
        return Err(anyhow::anyhow!("Database URL cannot be empty"));
    }

    // Validate server bind address
    if config.server.bind_address.is_empty() {
        return Err(anyhow::anyhow!("Server bind address cannot be empty"));
    }

    // Validate initial setup admin email format
    if config.initial_setup.setup_enabled && !config.initial_setup.setup_admin_email.contains('@') {
        return Err(anyhow::anyhow!("Initial setup admin email must be a valid email address"));
    }

    Ok(())
}

/// Load configuration from file or create default configuration
pub fn load_config<P: AsRef<Path>>(config_path: P) -> Result<AppConfig> {
    let path = config_path.as_ref();

    let config = if path.exists() {
        println!("Loading configuration from: {}", path.display());
        AppConfig::from_file(path)?
    } else {
        println!("Config file not found. Creating default configuration at: {}", path.display());
        let default_config = AppConfig::default();

        // Save default configuration to file
        default_config.to_file(path)
            .with_context(|| "Failed to create default configuration file")?;

        println!("Default configuration file created. Please review and modify as needed.");
        default_config
    };

    validate_config(&config)?;

    Ok(config)
}

/// Generate a sample configuration file with comments
pub fn generate_sample_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let default_config = AppConfig::default();
    
    default_config.to_file(&path)
        .with_context(|| "Failed to write sample configuration file")?;
    
    println!("Sample configuration file generated at: {}", path.as_ref().display());
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
