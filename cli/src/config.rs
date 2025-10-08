use anyhow::{Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Server URL to connect to
    pub server_url: String,
    /// Authentication token
    pub auth_token: Option<String>,
    /// Default output format
    pub output_format: String,
    /// Timeout for API requests (in seconds)
    pub timeout_seconds: u64,
    /// Enable request logging
    pub log_requests: bool,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:4399".to_string(),
            auth_token: None,
            output_format: "table".to_string(),
            timeout_seconds: 30,
            log_requests: false,
        }
    }
}

impl CliConfig {
    /// Load configuration from default location
    pub fn load_default() -> Result<Self> {
        let config_path = get_default_config_path()?;
        if config_path.exists() {
            Self::from_file(&config_path)
        } else {
            tracing::debug!("Config file not found at {:?}, using defaults", config_path);
            Ok(Self::default())
        }
    }

    /// Load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;

        let config: Self = toml::from_str(&content)
            .with_context(|| "Failed to parse TOML configuration")?;

        Ok(config)
    }

    /// Save configuration to file
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

    /// Save to default location
    pub fn save_default(&self) -> Result<()> {
        let config_path = get_default_config_path()?;
        self.to_file(&config_path)
    }
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Show current configuration
    Show,
    /// Generate a sample configuration file
    Generate {
        /// Output path for the configuration file
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Set configuration values
    Set {
        /// Configuration key to set
        key: String,
        /// Configuration value to set
        value: String,
    },
    /// Get configuration values
    Get {
        /// Configuration key to get
        key: String,
    },
}

pub async fn handle_config_command(command: ConfigCommand, config: &CliConfig) -> Result<()> {
    match command {
        ConfigCommand::Show => {
            println!("Current configuration:");
            println!("Server URL: {}", config.server_url);
            println!("Auth token: {}", config.auth_token.as_deref().unwrap_or("<not set>"));
            println!("Output format: {}", config.output_format);
            println!("Timeout: {} seconds", config.timeout_seconds);
            println!("Log requests: {}", config.log_requests);
            
            let config_path = get_default_config_path()?;
            println!("Config file: {}", config_path.display());
        }
        ConfigCommand::Generate { output } => {
            let path = output.unwrap_or_else(|| get_default_config_path().unwrap());
            let default_config = CliConfig::default();
            default_config.to_file(&path)?;
            println!("Generated configuration file at: {}", path.display());
        }
        ConfigCommand::Set { key, value } => {
            let mut config = CliConfig::load_default()?;
            let value_str = value.clone(); // Clone for later use in println
            match key.as_str() {
                "server_url" => config.server_url = value,
                "output_format" => config.output_format = value,
                "timeout_seconds" => {
                    config.timeout_seconds = value.parse()
                        .with_context(|| "Invalid timeout value")?;
                }
                "log_requests" => {
                    config.log_requests = value.parse()
                        .with_context(|| "Invalid boolean value")?;
                }
                _ => anyhow::bail!("Unknown configuration key: {}", key),
            }
            config.save_default()?;
            println!("Configuration updated: {} = {}", key, value_str);
        }
        ConfigCommand::Get { key } => {
            let config = CliConfig::load_default()?;
            let value = match key.as_str() {
                "server_url" => config.server_url,
                "output_format" => config.output_format,
                "timeout_seconds" => config.timeout_seconds.to_string(),
                "log_requests" => config.log_requests.to_string(),
                _ => anyhow::bail!("Unknown configuration key: {}", key),
            };
            println!("{}", value);
        }
    }
    Ok(())
}

fn get_default_config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Failed to determine config directory")?
        .join("css");
    
    Ok(config_dir.join("config.toml"))
}