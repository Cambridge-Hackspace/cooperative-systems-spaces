use anyhow::{Context, Result};
use console::style;
use serde_json::Value;

use crate::client::ApiClient;
use crate::config::CliConfig;

pub async fn handle_info_command(client: &ApiClient, config: &CliConfig) -> Result<()> {
    println!("{}", style("Server Information").bold().underlined());
    println!();

    // Get server info from the root endpoint
    let response = client.request_raw(reqwest::Method::GET, "/").await;
    
    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                if let Ok(text) = resp.text().await {
                    match config.output_format.as_str() {
                        "json" => {
                            println!("{}", text);
                        }
                        "yaml" => {
                            if let Ok(json) = serde_json::from_str::<Value>(&text) {
                                println!("{}", serde_yaml::to_string(&json)?);
                            } else {
                                println!("{}", text);
                            }
                        }
                        _ => {
                            if let Ok(json) = serde_json::from_str::<Value>(&text) {
                                print_server_info_formatted(&json);
                            } else {
                                println!("Raw response: {}", text);
                            }
                        }
                    }
                }
            } else {
                eprintln!("Failed to get server info: HTTP {}", resp.status());
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to server: {}", e);
        }
    }

    println!();
    
    // Show CLI information
    println!("{}", style("CLI Information").bold());
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!("Build date: {}", "unknown");
    println!("Git commit: {}", "unknown");

    Ok(())
}

fn print_server_info_formatted(json: &Value) {
    if let Some(status) = json.get("status") {
        println!("Status: {}", 
            if status == "ok" { 
                style("OK").green() 
            } else { 
                style(status.as_str().unwrap_or("unknown")).red() 
            }
        );
    }

    if let Some(site_name) = json.get("site_name") {
        println!("Site Name: {}", style(site_name.as_str().unwrap_or("unknown")).cyan());
    }

    if let Some(version) = json.get("version") {
        println!("Server Version: {}", style(version.as_str().unwrap_or("unknown")).yellow());
    }

    if let Some(uptime) = json.get("uptime") {
        println!("Uptime: {}", uptime.as_str().unwrap_or("unknown"));
    }

    if let Some(build_info) = json.get("build_info") {
        println!("Build Info:");
        if let Some(commit) = build_info.get("commit") {
            println!("  Git Commit: {}", commit.as_str().unwrap_or("unknown"));
        }
        if let Some(build_date) = build_info.get("build_date") {
            println!("  Build Date: {}", build_date.as_str().unwrap_or("unknown"));
        }
    }

    if let Some(features) = json.get("features") {
        println!("Features:");
        if let Some(features_obj) = features.as_object() {
            for (key, value) in features_obj {
                let enabled = value.as_bool().unwrap_or(false);
                println!("  {}: {}", 
                    key,
                    if enabled { 
                        style("enabled").green() 
                    } else { 
                        style("disabled").dim() 
                    }
                );
            }
        }
    }
}