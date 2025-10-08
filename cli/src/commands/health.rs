use anyhow::{Context, Result};
use console::style;
use reqwest::Method;
use serde_json::Value;

use crate::client::ApiClient;
use crate::config::CliConfig;
use crate::output;

pub async fn handle_health_command(client: &ApiClient, config: &CliConfig) -> Result<()> {
    println!("{}", style("Health Check").bold().underlined());
    println!();

    // Test basic connectivity
    println!("Testing server connectivity...");
    let response = client.request_raw(Method::GET, "/").await;
    
    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                output::print_success(&format!("✓ Server reachable ({})", resp.status()));
                
                // Try to parse the response
                if let Ok(text) = resp.text().await {
                    if let Ok(json) = serde_json::from_str::<Value>(&text) {
                        if let Some(status) = json.get("status") {
                            println!("  Server status: {}", status);
                        }
                        if let Some(version) = json.get("version") {
                            println!("  Server version: {}", version);
                        }
                        if let Some(site_name) = json.get("site_name") {
                            println!("  Site name: {}", site_name);
                        }
                    }
                }
            } else {
                output::print_error(&format!("✗ Server error ({})", resp.status()));
            }
        }
        Err(e) => {
            output::print_error(&format!("✗ Server unreachable: {}", e));
        }
    }

    println!();

    // Test API endpoint
    println!("Testing API connectivity...");
    let api_response = client.request_raw(Method::GET, "/api/").await;
    
    match api_response {
        Ok(resp) => {
            if resp.status().is_success() || resp.status().as_u16() == 404 {
                // 404 is expected if there's no API root endpoint
                output::print_success("✓ API endpoint accessible");
            } else {
                output::print_error(&format!("✗ API error ({})", resp.status()));
            }
        }
        Err(e) => {
            output::print_error(&format!("✗ API unreachable: {}", e));
        }
    }

    println!();

    // Test authentication if token is available
    if config.auth_token.is_some() {
        println!("Testing authentication...");
        let auth_response = client.request_raw(Method::GET, "/api/auth/me").await;
        
        match auth_response {
            Ok(resp) => {
                if resp.status().is_success() {
                    output::print_success("✓ Authentication valid");
                    
                    // Try to get user info
                    if let Ok(text) = resp.text().await {
                        if let Ok(json) = serde_json::from_str::<Value>(&text) {
                            if let Some(data) = json.get("data") {
                                if let Some(username) = data.get("username") {
                                    println!("  Authenticated as: {}", username);
                                }
                                if let Some(role) = data.get("role") {
                                    println!("  Role: {}", role);
                                }
                            }
                        }
                    }
                } else if resp.status().as_u16() == 401 {
                    output::print_error("✗ Authentication invalid (token expired or revoked)");
                } else {
                    output::print_error(&format!("✗ Authentication error ({})", resp.status()));
                }
            }
            Err(e) => {
                output::print_error(&format!("✗ Authentication check failed: {}", e));
            }
        }
    } else {
        output::print_warning("⚠ No authentication token (not logged in)");
        println!("  Use 'css auth login' to authenticate");
    }

    println!();

    // Show configuration summary
    println!("{}", style("Configuration").bold());
    println!("Server URL: {}", config.server_url);
    println!("Timeout: {} seconds", config.timeout_seconds);
    println!("Output format: {}", config.output_format);
    println!("Request logging: {}", if config.log_requests { "enabled" } else { "disabled" });

    Ok(())
}