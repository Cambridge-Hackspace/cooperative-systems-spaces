use anyhow::{Context, Result};
use clap::Subcommand;
use console::style;
use dialoguer::{Input, Password};

use crate::client::{ApiClient, ApiResponse, LoginRequest, LoginResponse, RegisterRequest, UserResponse};
use crate::config::CliConfig;
use crate::output;

#[derive(Subcommand)]
pub enum AuthCommand {
    /// Login to the server
    Login {
        /// Username or email
        #[arg(short, long)]
        username: Option<String>,
        /// Password (will prompt if not provided)
        #[arg(short, long)]
        password: Option<String>,
    },
    /// Logout (clear stored token)
    Logout,
    /// Register a new account
    Register {
        /// Username
        #[arg(short, long)]
        username: Option<String>,
        /// Email address
        #[arg(short, long)]
        email: Option<String>,
        /// Full name
        #[arg(short, long)]
        full_name: Option<String>,
        /// Password (will prompt if not provided)
        #[arg(short, long)]
        password: Option<String>,
    },
    /// Show current user information
    Whoami,
    /// Check if current token is valid
    Check,
}

pub async fn handle_auth_command(
    command: AuthCommand,
    client: &ApiClient,
    config: &mut CliConfig,
) -> Result<()> {
    match command {
        AuthCommand::Login { username, password } => {
            handle_login(client, config, username, password).await
        }
        AuthCommand::Logout => handle_logout(config).await,
        AuthCommand::Register {
            username,
            email,
            full_name,
            password,
        } => handle_register(client, username, email, full_name, password).await,
        AuthCommand::Whoami => handle_whoami(client, config).await,
        AuthCommand::Check => handle_check(client, config).await,
    }
}

async fn handle_login(
    client: &ApiClient,
    config: &mut CliConfig,
    username: Option<String>,
    password: Option<String>,
) -> Result<()> {
    let username = match username {
        Some(u) => u,
        None => Input::<String>::new()
            .with_prompt("Username or email")
            .interact_text()
            .context("Failed to read username")?,
    };

    let password = match password {
        Some(p) => p,
        None => Password::new()
            .with_prompt("Password")
            .interact()
            .context("Failed to read password")?,
    };

    println!("Logging in...");

    let login_request = LoginRequest {
        username_or_email: username,
        password,
    };

    let response: ApiResponse<LoginResponse> = client
        .post("/api/auth/login", &login_request)
        .await
        .context("Login request failed")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Login failed: {}", error);
    }

    let login_data = response.data.context("No login data in response")?;

    // Store the token
    config.auth_token = Some(login_data.token);
    config.save_default().context("Failed to save configuration")?;

    println!("{}", style("Login successful!").green());
    println!("Logged in as: {} ({})", login_data.user.full_name, login_data.user.username);
    println!("Role: {}", login_data.user.role);
    println!("Token expires in: {} seconds", login_data.expires_in);

    Ok(())
}

async fn handle_logout(config: &mut CliConfig) -> Result<()> {
    config.auth_token = None;
    config.save_default().context("Failed to save configuration")?;
    
    println!("{}", style("Logged out successfully").green());
    Ok(())
}

async fn handle_register(
    client: &ApiClient,
    username: Option<String>,
    email: Option<String>,
    full_name: Option<String>,
    password: Option<String>,
) -> Result<()> {
    let username = match username {
        Some(u) => u,
        None => Input::<String>::new()
            .with_prompt("Username")
            .interact_text()
            .context("Failed to read username")?,
    };

    let email = match email {
        Some(e) => e,
        None => Input::<String>::new()
            .with_prompt("Email")
            .interact_text()
            .context("Failed to read email")?,
    };

    let full_name = match full_name {
        Some(n) => n,
        None => Input::<String>::new()
            .with_prompt("Full name")
            .interact_text()
            .context("Failed to read full name")?,
    };

    let password = match password {
        Some(p) => p,
        None => {
            let pass1 = Password::new()
                .with_prompt("Password")
                .interact()
                .context("Failed to read password")?;
            
            let pass2 = Password::new()
                .with_prompt("Confirm password")
                .interact()
                .context("Failed to read password confirmation")?;

            if pass1 != pass2 {
                anyhow::bail!("Passwords do not match");
            }

            pass1
        }
    };

    println!("Registering account...");

    let register_request = RegisterRequest {
        username,
        email,
        password,
        full_name,
    };

    let response: ApiResponse<UserResponse> = client
        // `/api/auth/register`, not `/auth/register`. Every other call in this
        // crate carries the prefix; this one did not, so it never reached the
        // API at all -- it fell through to the server's static-file fallback.
        // checks/tests/cli_api_paths.rs now asserts the convention holds.
        .post("/api/auth/register", &register_request)
        .await
        .context("Registration request failed")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Registration failed: {}", error);
    }

    let user_data = response.data.context("No user data in response")?;

    println!("{}", style("Registration successful!").green());
    println!("Account created for: {} ({})", user_data.full_name, user_data.username);
    println!("Role: {}", user_data.role);
    println!("\nYou can now login with: css auth login");

    Ok(())
}

async fn handle_whoami(client: &ApiClient, config: &CliConfig) -> Result<()> {
    if config.auth_token.is_none() {
        println!("{}", style("Not logged in").red());
        println!("Use 'css auth login' to authenticate");
        return Ok(());
    }

    let response: ApiResponse<UserResponse> = client
        .get("/api/auth/me")
        .await
        .context("Failed to get current user information")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to get user info: {}", error);
    }

    let user = response.data.context("No user data in response")?;

    match config.output_format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&user)?);
        }
        "yaml" => {
            println!("{}", serde_yaml::to_string(&user)?);
        }
        _ => {
            output::print_user_info(&user);
        }
    }

    Ok(())
}

async fn handle_check(client: &ApiClient, config: &CliConfig) -> Result<()> {
    if config.auth_token.is_none() {
        println!("{}", style("No authentication token").red());
        return Ok(());
    }

    match client.get::<ApiResponse<UserResponse>>("/api/auth/me").await {
        Ok(response) if response.success => {
            println!("{}", style("Token is valid").green());
            if let Some(user) = response.data {
                println!("Authenticated as: {} ({})", user.full_name, user.username);
            }
        }
        Ok(_) => {
            println!("{}", style("Token is invalid").red());
        }
        Err(e) => {
            println!("{}", style("Token is invalid or server unreachable").red());
            tracing::debug!("Error details: {}", e);
        }
    }

    Ok(())
}