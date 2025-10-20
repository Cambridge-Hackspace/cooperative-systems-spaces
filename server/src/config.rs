use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, RwLock};
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
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            jwt_secret: "your-super-secret-jwt-key-change-this-in-production".to_string(),
            jwt_expiration_hours: 24,
            allow_registration: true,
            require_email_verification: false,
            password_min_length: 8,
            session_timeout_minutes: 1440, // 24 hours
            password_reset_enabled: true,
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
            bind_address: "127.0.0.1:4399".to_string(),
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
    /// Profile field definitions
    pub profile_fields: Vec<ProfileField>,
    /// Enable user profile functionality
    pub profiles_enabled: bool,
}

/// Profile field type specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileFieldType {
    Text,
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
            profile_fields: vec![
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
            profiles_enabled: true,
        }
    }
}


/// Toolpass Compatability Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPassConfig {
    /// is it enabled?
    pub enabled: bool,
    /// what profile field we should pull this out of
    pub profile_field: String,
    /// global-api-key
    pub global_api_key: Option<String>,
}

impl Default for ToolPassConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            profile_field: "card_id".to_string(),
            global_api_key: None
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
    /// Toolpass Compat Configuration
    pub toolpass: ToolPassConfig,
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
            toolpass: ToolPassConfig::default(),
        }
    }
}

impl AppConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        // Try to parse the configuration
        let config_result: Result<AppConfig, toml::de::Error> = toml::from_str(&content);
        
        let mut config = match config_result {
            Ok(cfg) => cfg,
            Err(e) => {
                // Check if error is about missing fields
                let error_msg = e.to_string();
                if error_msg.contains("missing field") {
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

        let new_config = AppConfig::from_file(config_path)
            .with_context(|| "Failed to reload configuration")?;

        // Validate the new configuration
        self.validate_config(&new_config)?;

        // Update the configuration atomically
        {
            let mut config_guard = self.config.write().unwrap();
            *config_guard = new_config;
        }

        info!("Configuration reloaded successfully");
        Ok(())
    }

    /// Validate configuration before applying
    fn validate_config(&self, config: &AppConfig) -> Result<()> {
        // Validate JWT secret is not default
        if config.auth.jwt_secret == "your-super-secret-jwt-key-change-this-in-production" {
            warn!("JWT secret is still set to default value - this is insecure for production");
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

    /// Get a thread-safe reference to the configuration
    pub fn get_config_ref(&self) -> Arc<RwLock<AppConfig>> {
        Arc::clone(&self.config)
    }
}

/// Load configuration from file or create default configuration
pub fn load_config<P: AsRef<Path>>(config_path: P) -> Result<AppConfig> {
    let path = config_path.as_ref();
    
    if path.exists() {
        println!("Loading configuration from: {}", path.display());
        AppConfig::from_file(path)
    } else {
        println!("Config file not found. Creating default configuration at: {}", path.display());
        let default_config = AppConfig::default();
        
        // Save default configuration to file
        default_config.to_file(path)
            .with_context(|| "Failed to create default configuration file")?;
        
        println!("Default configuration file created. Please review and modify as needed.");
        Ok(default_config)
    }
}

/// Generate a sample configuration file with comments
pub fn generate_sample_config<P: AsRef<Path>>(path: P) -> Result<()> {
    let sample_config = include_str!("../../config.sample.toml");
    
    fs::write(&path, sample_config)
        .with_context(|| format!("Failed to write sample config file: {}", path.as_ref().display()))?;
    
    println!("Sample configuration file generated at: {}", path.as_ref().display());
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
}
