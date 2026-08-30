use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use css_cli::client::ApiClient;
use css_cli::config::CliConfig;
use css_cli::{auth, commands, config};

/// Cooperative Systems Spaces CLI - Administrative and management tool
#[derive(Parser)]
#[command(name = "css", version, about, long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(long, short = 'c')]
    config: Option<PathBuf>,

    /// Server URL to connect to
    #[arg(long, short = 's', env = "CSS_SERVER_URL")]
    server: Option<String>,

    /// API token for authentication
    #[arg(long, short = 't', env = "CSS_TOKEN")]
    token: Option<String>,

    /// Output format (json, table, yaml)
    #[arg(long, short = 'o', default_value = "table")]
    output: String,

    /// Enable verbose output
    #[arg(long, short = 'v', action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: config::ConfigCommand,
    },
    /// Authentication and login
    Auth {
        #[command(subcommand)]
        action: auth::AuthCommand,
    },
    /// User management
    Users {
        #[command(subcommand)]
        action: commands::users::UserCommand,
    },
    /// System health and diagnostics
    Health,
    /// Server information
    Info,
    // `commands::admin` was fully implemented and declared in
    // `commands/mod.rs`, but this enum had no variant for it, so
    // `css-cli admin reload-config` did not exist and the whole module read as
    // dead code. Kept as a line comment, not a doc comment: clap renders doc
    // comments as user-facing help text.
    /// Administrative operations against the server
    Admin {
        #[command(subcommand)]
        action: commands::admin::AdminCommand,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose);

    // Load configuration
    let mut config = if let Some(config_path) = cli.config {
        CliConfig::from_file(&config_path)
            .with_context(|| format!("Failed to load config from {}", config_path.display()))?
    } else {
        CliConfig::load_default()?
    };

    // Override config with CLI arguments
    if let Some(server) = cli.server {
        config.server_url = server;
    }
    if let Some(token) = cli.token {
        config.auth_token = Some(token);
    }
    config.output_format = cli.output.clone();

    // Create API client
    let client = ApiClient::new(&config).context("Failed to create API client")?;

    // Execute command
    match cli.command {
        Commands::Config { action } => config::handle_config_command(action, &config).await,
        Commands::Auth { action } => auth::handle_auth_command(action, &client, &mut config).await,
        Commands::Users { action } => {
            commands::users::handle_user_command(action, &client, &config).await
        }
        Commands::Health => commands::health::handle_health_command(&client, &config).await,
        Commands::Info => commands::info::handle_info_command(&client, &config).await,
        Commands::Admin { action } => {
            commands::admin::handle_admin_command(action, &client, &config).await
        }
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        2 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();
}
