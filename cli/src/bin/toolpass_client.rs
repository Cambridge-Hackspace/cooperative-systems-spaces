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

    /// API key for authentication (can also be set via TOOLPASS_API_KEY env var)
    #[arg(short, long, env = "TOOLPASS_API_KEY")]
    api_key: Option<String>,

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
        tool_id: String,
    },

    /// Simulate tool deactivation
    ToolOff {
        /// Card/RFID identifier
        #[arg(short, long)]
        card: String,

        /// Tool ID
        #[arg(short, long)]
        tool_id: String,
    },

    /// Log tool usage with duration
    Log {
        /// Card/RFID identifier
        #[arg(short, long)]
        card: String,

        /// Tool ID
        #[arg(short, long)]
        tool_id: String,

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
        tool_id: String,

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
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;

    match cli.command {
        Commands::Status => {
            check_status(&client, &cli.server).await?;
        }
        Commands::ToolOn { card, tool_id } => {
            tool_on(
                &client,
                &cli.server,
                cli.api_key.as_deref(),
                &card,
                &tool_id,
            )
            .await?;
        }
        Commands::ToolOff { card, tool_id } => {
            tool_off(
                &client,
                &cli.server,
                cli.api_key.as_deref(),
                &card,
                &tool_id,
            )
            .await?;
        }
        Commands::Log {
            card,
            tool_id,
            seconds,
            temperature,
        } => {
            tool_log(
                &client,
                &cli.server,
                cli.api_key.as_deref(),
                &card,
                &tool_id,
                seconds,
                temperature,
            )
            .await?;
        }
        Commands::Session {
            card,
            tool_id,
            duration,
            temperature,
        } => {
            simulate_session(
                &client,
                &cli.server,
                cli.api_key.as_deref(),
                &card,
                &tool_id,
                duration,
                temperature,
            )
            .await?;
        }
        Commands::AddUser {
            api_key,
            email,
            first_name,
            last_name,
        } => {
            add_user(&client, &cli.server, api_key, email, first_name, last_name).await?;
        }
        Commands::RemoveUser { api_key, email } => {
            remove_user(&client, &cli.server, api_key, email).await?;
        }
    }

    Ok(())
}

/// Append `&api_key=<key>` when one was supplied.
///
/// The CLI has always accepted `--api-key` / `TOOLPASS_API_KEY` and never sent
/// it anywhere. The other half of the same mechanism was equally inert: the
/// server parsed an `api_key` field on these requests and never read it, and
/// `validate_api_key` was called from nowhere. Both halves were written; the
/// wire between them was not. Now that the server honours it, this is what
/// makes an api-key-authenticated controller work.
fn with_api_key(url: &str, api_key: Option<&str>) -> String {
    match api_key.filter(|k| !k.is_empty()) {
        Some(key) => format!("{url}&api_key={key}"),
        None => url.to_string(),
    }
}

async fn check_status(client: &Client, server: &str) -> Result<()> {
    println!("🔍 Checking ToolPass API status...");

    let url = format!("{}/api/toolguard/", server);
    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    let status = response.status();
    let body: ToolPassResponse = response.json().await?;

    println!("✅ Status: {} (HTTP {})", body.status, status);
    println!("   API Version: {}", body.api_version);

    Ok(())
}

async fn tool_on(
    client: &Client,
    server: &str,
    api_key: Option<&str>,
    card: &str,
    tool_id: &str,
) -> Result<()> {
    println!("🔓 Requesting tool activation...");
    println!("   Card: {}", card);
    println!("   Tool ID: {}", tool_id);

    let url = with_api_key(
        &format!(
            "{}/api/toolguard/tool-on?card={}&tool_id={}",
            server, card, tool_id
        ),
        api_key,
    );
    println!("   URL: {}", url);

    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    println!("   Response status: {}", response.status());

    // Debug: print raw response body
    let response_text = response.text().await?;
    println!("   Response body: {}", response_text);

    let body: ToolPassResponse =
        serde_json::from_str(&response_text).context("Failed to parse JSON response")?;

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

async fn tool_off(
    client: &Client,
    server: &str,
    api_key: Option<&str>,
    card: &str,
    tool_id: &str,
) -> Result<()> {
    println!("🔒 Deactivating tool...");
    println!("   Card: {}", card);
    println!("   Tool ID: {}", tool_id);

    let url = with_api_key(
        &format!(
            "{}/api/toolguard/tool-off?card={}&tool_id={}",
            server, card, tool_id
        ),
        api_key,
    );
    let response = client
        .get(&url)
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
    api_key: Option<&str>,
    card: &str,
    tool_id: &str,
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
        "{}/api/toolguard/tool-log?card={}&tool_id={}&seconds={}",
        server, card, tool_id, seconds
    );

    if let Some(temp) = temperature {
        url.push_str(&format!("&temperature={}", temp));
    }
    let url = with_api_key(&url, api_key);

    let response = client
        .get(&url)
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
    api_key: Option<&str>,
    card: &str,
    tool_id: &str,
    duration: u64,
    temperature: Option<f32>,
) -> Result<()> {
    println!("🎬 Simulating complete tool usage session...");
    println!();

    // Step 1: Tool On
    tool_on(client, server, api_key, card, &tool_id).await?;

    println!();
    println!("⏳ Using tool for {} seconds...", duration);
    tokio::time::sleep(Duration::from_secs(duration)).await;

    println!();

    // Step 2: Tool Off
    tool_off(client, server, api_key, card, &tool_id).await?;

    println!();

    // Step 3: Log usage
    tool_log(
        client,
        server,
        api_key,
        card,
        &tool_id,
        duration as f32,
        temperature,
    )
    .await?;

    println!();
    println!("✅ Session complete!");

    Ok(())
}

// NOTE: `/api/toolpass/v1/add-user` and `/api/toolpass/v1/remove-user` below
// have NO counterpart on the server -- there is no `/api/toolpass` router
// anywhere in this workspace, and no add-user/remove-user endpoint under
// `/api/toolguard` either. They are left pointing at the path that does not
// exist rather than being re-aimed at something plausible: inventing a target
// would hide the fact that the feature was never built server-side, and a 404
// naming a path nobody serves is a more useful thing to hit than a 404 naming
// one that looks like it should work.
//
// checks/tests/cli_api_paths.rs holds them on an explicit unresolved list, so
// this cannot be forgotten and cannot spread.
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

    let response = client
        .post(&url)
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

async fn remove_user(client: &Client, server: &str, api_key: String, email: String) -> Result<()> {
    println!("➖ Removing user...");
    println!("   Email: {}", email);

    let url = format!("{}/api/toolpass/v1/remove-user", server);
    let req = RemoveUserRequest { api_key, email };

    let response = client
        .post(&url)
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
