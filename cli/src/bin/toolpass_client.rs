//! ToolPass Device Simulator
//! 
//! This binary simulates an IoT device (like an Arduino with RFID reader)
//! that makes requests to the ToolPass API.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "toolpass-client")]
#[command(about = "ToolPass device simulator", long_about = None)]
struct Cli {
    /// Server URL (e.g., http://localhost:3000)
    #[arg(short, long, default_value = "http://localhost:4399")]
    server: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check API status
    Status,
    
    /// Simulate tool activation (card tap on RFID reader)
    ToolOn {
        /// Card/RFID identifier (username or email)
        #[arg(short, long)]
        card: String,
        
        /// Tool ID
        #[arg(short, long)]
        tool_id: i32,
    },
    
    /// Simulate tool deactivation
    ToolOff {
        /// Card/RFID identifier
        #[arg(short, long)]
        card: String,
        
        /// Tool ID
        #[arg(short, long)]
        tool_id: i32,
    },
    
    /// Log tool usage with duration
    Log {
        /// Card/RFID identifier
        #[arg(short, long)]
        card: String,
        
        /// Tool ID
        #[arg(short, long)]
        tool_id: i32,
        
        /// Usage duration in seconds
        #[arg(short, long)]
        seconds: f32,
        
        /// Optional temperature reading
        #[arg(short = 'T', long)]
        temperature: Option<f32>,
    },
    
    /// Simulate a complete tool usage session
    Session {
        /// Card/RFID identifier
        #[arg(short, long)]
        card: String,
        
        /// Tool ID
        #[arg(short, long)]
        tool_id: i32,
        
        /// Session duration in seconds
        #[arg(short, long, default_value = "60")]
        duration: u64,
        
        /// Temperature reading (optional)
        #[arg(short = 'T', long)]
        temperature: Option<f32>,
    },
    
    /// Add a user via API
    AddUser {
        /// API key
        #[arg(short, long)]
        api_key: String,
        
        /// Email
        #[arg(short, long)]
        email: String,
        
        /// First name
        #[arg(short, long)]
        first_name: String,
        
        /// Last name
        #[arg(short, long)]
        last_name: String,
    },
    
    /// Remove a user via API
    RemoveUser {
        /// API key
        #[arg(short, long)]
        api_key: String,
        
        /// Email
        #[arg(short, long)]
        email: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolPassResponse {
    api_version: f32,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_off: Option<bool>,
}

#[derive(Debug, Serialize)]
struct AddUserRequest {
    api_key: String,
    email: String,
    first_name: String,
    last_name: String,
}

#[derive(Debug, Serialize)]
struct RemoveUserRequest {
    api_key: String,
    email: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    match cli.command {
        Commands::Status => {
            check_status(&client, &cli.server).await?;
        }
        Commands::ToolOn { card, tool_id } => {
            tool_on(&client, &cli.server, &card, tool_id).await?;
        }
        Commands::ToolOff { card, tool_id } => {
            tool_off(&client, &cli.server, &card, tool_id).await?;
        }
        Commands::Log { card, tool_id, seconds, temperature } => {
            tool_log(&client, &cli.server, &card, tool_id, seconds, temperature).await?;
        }
        Commands::Session { card, tool_id, duration, temperature } => {
            simulate_session(&client, &cli.server, &card, tool_id, duration, temperature).await?;
        }
        Commands::AddUser { api_key, email, first_name, last_name } => {
            add_user(&client, &cli.server, api_key, email, first_name, last_name).await?;
        }
        Commands::RemoveUser { api_key, email } => {
            remove_user(&client, &cli.server, api_key, email).await?;
        }
    }

    Ok(())
}

async fn check_status(client: &Client, server: &str) -> Result<()> {
    println!("🔍 Checking ToolPass API status...");
    
    let url = format!("{}/api/toolpass/v1", server);
    let response = client.get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    let status = response.status();
    let body: ToolPassResponse = response.json().await?;

    println!("✅ Status: {} (HTTP {})", body.status, status);
    println!("   API Version: {}", body.api_version);
    
    Ok(())
}

async fn tool_on(client: &Client, server: &str, card: &str, tool_id: i32) -> Result<()> {
    println!("🔓 Requesting tool activation...");
    println!("   Card: {}", card);
    println!("   Tool ID: {}", tool_id);
    
    let url = format!("{}/api/toolpass/v1/tool-on?card={}&tool_id={}", server, card, tool_id);
    let response = client.get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    let body: ToolPassResponse = response.json().await?;

    match body.status.as_str() {
        "ok" => {
            if body.tool_on.unwrap_or(false) {
                println!("✅ Tool AUTHORIZED");
                if let Some(msg) = body.message {
                    println!("   {}", msg);
                }
            } else {
                println!("❌ Tool DENIED");
            }
        }
        "error" => {
            println!("❌ DENIED: {}", body.message.unwrap_or_default());
        }
        _ => {
            println!("⚠️  Unknown status: {}", body.status);
        }
    }
    
    Ok(())
}

async fn tool_off(client: &Client, server: &str, card: &str, tool_id: i32) -> Result<()> {
    println!("🔒 Deactivating tool...");
    println!("   Card: {}", card);
    println!("   Tool ID: {}", tool_id);
    
    let url = format!("{}/api/toolpass/v1/tool-off?card={}&tool_id={}", server, card, tool_id);
    let response = client.get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    let body: ToolPassResponse = response.json().await?;

    match body.status.as_str() {
        "ok" => {
            println!("✅ Tool deactivated");
            if let Some(msg) = body.message {
                println!("   {}", msg);
            }
        }
        "error" => {
            println!("❌ Error: {}", body.message.unwrap_or_default());
        }
        _ => {
            println!("⚠️  Unknown status: {}", body.status);
        }
    }
    
    Ok(())
}

async fn tool_log(
    client: &Client,
    server: &str,
    card: &str,
    tool_id: i32,
    seconds: f32,
    temperature: Option<f32>,
) -> Result<()> {
    println!("📝 Logging tool usage...");
    println!("   Card: {}", card);
    println!("   Tool ID: {}", tool_id);
    println!("   Duration: {:.1}s ({:.1} min)", seconds, seconds / 60.0);
    if let Some(temp) = temperature {
        println!("   Temperature: {:.1}°C", temp);
    }
    
    let mut url = format!(
        "{}/api/toolpass/v1/tool-log?card={}&tool_id={}&seconds={}",
        server, card, tool_id, seconds
    );
    
    if let Some(temp) = temperature {
        url.push_str(&format!("&temperature={}", temp));
    }
    
    let response = client.get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    let body: ToolPassResponse = response.json().await?;

    match body.status.as_str() {
        "ok" => {
            println!("✅ Usage logged successfully");
        }
        "error" => {
            println!("❌ Error: {}", body.message.unwrap_or_default());
        }
        _ => {
            println!("⚠️  Unknown status: {}", body.status);
        }
    }
    
    Ok(())
}

async fn simulate_session(
    client: &Client,
    server: &str,
    card: &str,
    tool_id: i32,
    duration: u64,
    temperature: Option<f32>,
) -> Result<()> {
    println!("🎬 Simulating complete tool usage session...");
    println!();
    
    // Step 1: Tool On
    tool_on(client, server, card, tool_id).await?;
    
    println!();
    println!("⏳ Using tool for {} seconds...", duration);
    tokio::time::sleep(Duration::from_secs(duration)).await;
    
    println!();
    
    // Step 2: Tool Off
    tool_off(client, server, card, tool_id).await?;
    
    println!();
    
    // Step 3: Log usage
    tool_log(client, server, card, tool_id, duration as f32, temperature).await?;
    
    println!();
    println!("✅ Session complete!");
    
    Ok(())
}

async fn add_user(
    client: &Client,
    server: &str,
    api_key: String,
    email: String,
    first_name: String,
    last_name: String,
) -> Result<()> {
    println!("➕ Adding user...");
    println!("   Email: {}", email);
    println!("   Name: {} {}", first_name, last_name);
    
    let url = format!("{}/api/toolpass/v1/add-user", server);
    let req = AddUserRequest {
        api_key,
        email,
        first_name,
        last_name,
    };
    
    let response = client.post(&url)
        .json(&req)
        .send()
        .await
        .context("Failed to send request")?;

    let body: ToolPassResponse = response.json().await?;

    match body.status.as_str() {
        "ok" => {
            println!("✅ User added successfully");
            if let Some(msg) = body.message {
                println!("   {}", msg);
            }
        }
        "error" => {
            println!("❌ Error: {}", body.message.unwrap_or_default());
        }
        _ => {
            println!("⚠️  Unknown status: {}", body.status);
        }
    }
    
    Ok(())
}

async fn remove_user(
    client: &Client,
    server: &str,
    api_key: String,
    email: String,
) -> Result<()> {
    println!("➖ Removing user...");
    println!("   Email: {}", email);
    
    let url = format!("{}/api/toolpass/v1/remove-user", server);
    let req = RemoveUserRequest {
        api_key,
        email,
    };
    
    let response = client.post(&url)
        .json(&req)
        .send()
        .await
        .context("Failed to send request")?;

    let body: ToolPassResponse = response.json().await?;

    match body.status.as_str() {
        "ok" => {
            println!("✅ User removed successfully");
            if let Some(msg) = body.message {
                println!("   {}", msg);
            }
        }
        "error" => {
            println!("❌ Error: {}", body.message.unwrap_or_default());
        }
        _ => {
            println!("⚠️  Unknown status: {}", body.status);
        }
    }
    
    Ok(())
}
