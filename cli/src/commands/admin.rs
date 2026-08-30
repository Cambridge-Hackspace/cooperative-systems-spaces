use anyhow::Result;
use clap::Subcommand;

use crate::client::{ApiClient, ApiResponse};
use crate::config::CliConfig;
use crate::output;

#[derive(Subcommand)]
pub enum AdminCommand {
    /// Reload server configuration from disk
    ReloadConfig,
}

pub async fn handle_admin_command(
    command: AdminCommand,
    client: &ApiClient,
    config: &CliConfig,
) -> Result<()> {
    match command {
        AdminCommand::ReloadConfig => handle_reload_config(client, config).await,
    }
}

async fn handle_reload_config(client: &ApiClient, config: &CliConfig) -> Result<()> {
    println!("Reloading server configuration...");

    let response: ApiResponse<serde_json::Value> = client
        .post("/api/admin/reload-config", &serde_json::json!({}))
        .await?;

    if response.success {
        output::print_success("Configuration reloaded successfully!");

        if let Some(data) = response.data {
            match config.output_format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&data)?);
                }
                "yaml" => {
                    println!("{}", serde_yaml::to_string(&data)?);
                }
                _ => {
                    println!("Updated configuration:");
                    if let Some(site_name) = data.get("site_name") {
                        println!("  Site name: {}", site_name.as_str().unwrap_or("unknown"));
                    }
                    if let Some(debug_mode) = data.get("debug_mode") {
                        println!("  Debug mode: {}", debug_mode.as_bool().unwrap_or(false));
                    }
                    if let Some(setup_enabled) = data.get("initial_setup_enabled") {
                        println!(
                            "  Initial setup: {}",
                            if setup_enabled.as_bool().unwrap_or(false) {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        );
                    }
                    if let Some(auth_config) = data.get("auth_config") {
                        println!("  Authentication:");
                        if let Some(allow_reg) = auth_config.get("allow_registration") {
                            println!(
                                "    Registration: {}",
                                if allow_reg.as_bool().unwrap_or(false) {
                                    "enabled"
                                } else {
                                    "disabled"
                                }
                            );
                        }
                        if let Some(email_verify) = auth_config.get("require_email_verification") {
                            println!(
                                "    Email verification: {}",
                                if email_verify.as_bool().unwrap_or(false) {
                                    "required"
                                } else {
                                    "not required"
                                }
                            );
                        }
                        if let Some(min_length) = auth_config.get("password_min_length") {
                            println!(
                                "    Min password length: {}",
                                min_length.as_u64().unwrap_or(8)
                            );
                        }
                    }
                }
            }
        }

        if let Some(message) = response.message {
            println!("\n{}", message);
        }
    } else {
        let error = response.error.unwrap_or("Unknown error".to_string());
        output::print_error(&format!("Failed to reload configuration: {}", error));
    }

    Ok(())
}
