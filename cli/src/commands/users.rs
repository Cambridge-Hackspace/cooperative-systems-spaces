use anyhow::{Context, Result};
use clap::Subcommand;
use console::style;
use uuid::Uuid;

use crate::client::{
    ApiClient, ApiResponse, PaginatedResponse, UpdateUserRequest, UserResponse, UserRole,
};
use crate::config::CliConfig;
use crate::output;

#[derive(Subcommand)]
pub enum UserCommand {
    /// List all users
    List {
        /// Page number (default: 1)
        #[arg(long, short, default_value = "1")]
        page: u32,
        /// Number of users per page (default: 20)
        #[arg(long, default_value = "20")]
        per_page: u32,
        /// Show only active users
        #[arg(long)]
        active_only: bool,
        /// Filter by role
        #[arg(long)]
        role: Option<String>,
    },
    /// Show details of a specific user
    Show {
        /// User ID or username
        user_id: String,
    },
    /// Create a new user (admin only)
    Create {
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
        /// User role
        #[arg(short, long)]
        role: Option<String>,
    },
    /// Update user information
    Update {
        /// User ID or username
        user_id: String,
        /// New username
        #[arg(long)]
        username: Option<String>,
        /// New email
        #[arg(long)]
        email: Option<String>,
        /// New full name
        #[arg(long)]
        full_name: Option<String>,
        /// New password (will prompt if flag provided without value)
        #[arg(long)]
        password: Option<String>,
        /// New role
        #[arg(long)]
        role: Option<String>,
        /// Activate/deactivate user
        #[arg(long)]
        active: Option<bool>,
    },
    /// Delete a user (admin only)
    Delete {
        /// User ID or username
        user_id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Promote user role
    Promote {
        /// User ID or username
        user_id: String,
    },
    /// Demote user role
    Demote {
        /// User ID or username
        user_id: String,
    },
}

pub async fn handle_user_command(
    command: UserCommand,
    client: &ApiClient,
    config: &CliConfig,
) -> Result<()> {
    match command {
        UserCommand::List {
            page,
            per_page,
            active_only: _,
            role: _,
        } => handle_list_users(client, config, page, per_page).await,
        UserCommand::Show { user_id } => handle_show_user(client, config, &user_id).await,
        UserCommand::Create {
            username,
            email,
            full_name,
            password,
            role,
        } => handle_create_user(client, username, email, full_name, password, role).await,
        UserCommand::Update {
            user_id,
            username,
            email,
            full_name,
            password,
            role,
            active,
        } => {
            handle_update_user(
                client, &user_id, username, email, full_name, password, role, active,
            )
            .await
        }
        UserCommand::Delete { user_id, yes } => handle_delete_user(client, &user_id, yes).await,
        UserCommand::Promote { user_id } => handle_promote_user(client, &user_id).await,
        UserCommand::Demote { user_id } => handle_demote_user(client, &user_id).await,
    }
}

async fn handle_list_users(
    client: &ApiClient,
    config: &CliConfig,
    page: u32,
    per_page: u32,
) -> Result<()> {
    let path = format!("/api/users?page={}&per_page={}", page, per_page);
    let response: ApiResponse<PaginatedResponse<UserResponse>> =
        client.get(&path).await.context("Failed to list users")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to list users: {}", error);
    }

    let data = response.data.context("No data in response")?;

    match config.output_format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        "yaml" => {
            println!("{}", serde_yaml::to_string(&data)?);
        }
        _ => {
            println!(
                "Users (page {} of {}, {} total)",
                data.page, data.total_pages, data.total
            );
            println!();
            output::print_users_table(&data.items);
        }
    }

    Ok(())
}

async fn handle_show_user(client: &ApiClient, config: &CliConfig, user_id: &str) -> Result<()> {
    let path = if user_id.contains('-') {
        // Assume it's a UUID
        format!("/api/users/{}", user_id)
    } else {
        // Try to find user by username first
        let users_response: ApiResponse<PaginatedResponse<UserResponse>> =
            client.get("/api/users?per_page=100").await?;

        if let Some(data) = users_response.data {
            if let Some(user) = data.items.iter().find(|u| u.username == user_id) {
                format!("/api/users/{}", user.id)
            } else {
                anyhow::bail!("User not found: {}", user_id);
            }
        } else {
            anyhow::bail!("Failed to search for user");
        }
    };

    let response: ApiResponse<UserResponse> =
        client.get(&path).await.context("Failed to get user")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to get user: {}", error);
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

async fn handle_create_user(
    _client: &ApiClient,
    _username: Option<String>,
    _email: Option<String>,
    _full_name: Option<String>,
    _password: Option<String>,
    _role: Option<String>,
) -> Result<()> {
    // For now, direct user creation by admin is not implemented
    // Users should register through the registration endpoint
    println!(
        "{}",
        style("User creation through CLI not yet implemented").yellow()
    );
    println!("Users should register using: css auth register");
    Ok(())
}

async fn handle_update_user(
    client: &ApiClient,
    user_id: &str,
    username: Option<String>,
    email: Option<String>,
    full_name: Option<String>,
    password: Option<String>,
    role: Option<String>,
    active: Option<bool>,
) -> Result<()> {
    // First, resolve the user ID
    let user_uuid = resolve_user_id(client, user_id).await?;

    let role_enum = if let Some(role_str) = role {
        Some(parse_role(&role_str)?)
    } else {
        None
    };

    let update_request = UpdateUserRequest {
        username,
        email,
        full_name,
        password,
        is_active: active,
        role: role_enum,
    };

    let path = format!("/api/users/{}", user_uuid);
    let response: ApiResponse<UserResponse> = client
        .put(&path, &update_request)
        .await
        .context("Failed to update user")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to update user: {}", error);
    }

    let user = response.data.context("No user data in response")?;
    output::print_success("User updated successfully");
    output::print_user_info(&user);

    Ok(())
}

async fn handle_delete_user(client: &ApiClient, user_id: &str, yes: bool) -> Result<()> {
    let user_uuid = resolve_user_id(client, user_id).await?;

    if !yes {
        let confirm = dialoguer::Confirm::new()
            .with_prompt(&format!(
                "Are you sure you want to delete user {}?",
                user_id
            ))
            .default(false)
            .interact()
            .context("Failed to get confirmation")?;

        if !confirm {
            println!("Aborted");
            return Ok(());
        }
    }

    let path = format!("/api/users/{}", user_uuid);
    let response: ApiResponse<()> = client
        .delete(&path)
        .await
        .context("Failed to delete user")?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to delete user: {}", error);
    }

    output::print_success(&format!("User {} deleted successfully", user_id));
    Ok(())
}

async fn handle_promote_user(client: &ApiClient, user_id: &str) -> Result<()> {
    let user_uuid = resolve_user_id(client, user_id).await?;

    // Get current user info
    let path = format!("/api/users/{}", user_uuid);
    let response: ApiResponse<UserResponse> = client.get(&path).await?;
    let user = response.data.context("No user data in response")?;

    let new_role = match user.role {
        UserRole::Unknown => UserRole::Newbie,
        UserRole::Newbie => UserRole::Member,
        UserRole::Member => UserRole::Staff,
        UserRole::Staff => UserRole::Admin,
        UserRole::Admin => {
            output::print_warning("User is already at maximum role (Admin)");
            return Ok(());
        }
    };

    let update_request = UpdateUserRequest {
        username: None,
        email: None,
        full_name: None,
        password: None,
        is_active: None,
        role: Some(new_role),
    };

    let response: ApiResponse<UserResponse> = client.put(&path, &update_request).await?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to promote user: {}", error);
    }

    let updated_user = response.data.context("No user data in response")?;
    output::print_success(&format!(
        "User {} promoted from {} to {}",
        user_id, user.role, updated_user.role
    ));

    Ok(())
}

async fn handle_demote_user(client: &ApiClient, user_id: &str) -> Result<()> {
    let user_uuid = resolve_user_id(client, user_id).await?;

    // Get current user info
    let path = format!("/api/users/{}", user_uuid);
    let response: ApiResponse<UserResponse> = client.get(&path).await?;
    let user = response.data.context("No user data in response")?;

    let new_role = match user.role {
        UserRole::Admin => UserRole::Staff,
        UserRole::Staff => UserRole::Member,
        UserRole::Member => UserRole::Newbie,
        UserRole::Newbie => UserRole::Unknown,
        UserRole::Unknown => {
            output::print_warning("User is already at minimum role (Unknown)");
            return Ok(());
        }
    };

    let update_request = UpdateUserRequest {
        username: None,
        email: None,
        full_name: None,
        password: None,
        is_active: None,
        role: Some(new_role),
    };

    let response: ApiResponse<UserResponse> = client.put(&path, &update_request).await?;

    if !response.success {
        let error = response.error.unwrap_or("Unknown error".to_string());
        anyhow::bail!("Failed to demote user: {}", error);
    }

    let updated_user = response.data.context("No user data in response")?;
    output::print_success(&format!(
        "User {} demoted from {} to {}",
        user_id, user.role, updated_user.role
    ));

    Ok(())
}

async fn resolve_user_id(client: &ApiClient, user_id: &str) -> Result<Uuid> {
    if let Ok(uuid) = Uuid::parse_str(user_id) {
        return Ok(uuid);
    }

    // Search by username
    let users_response: ApiResponse<PaginatedResponse<UserResponse>> =
        client.get("/api/users?per_page=100").await?;

    if let Some(data) = users_response.data {
        if let Some(user) = data.items.iter().find(|u| u.username == user_id) {
            Ok(user.id)
        } else {
            anyhow::bail!("User not found: {}", user_id);
        }
    } else {
        anyhow::bail!("Failed to search for user");
    }
}

fn parse_role(role_str: &str) -> Result<UserRole> {
    match role_str.to_lowercase().as_str() {
        "unknown" => Ok(UserRole::Unknown),
        "newbie" => Ok(UserRole::Newbie),
        "member" => Ok(UserRole::Member),
        "staff" => Ok(UserRole::Staff),
        "admin" => Ok(UserRole::Admin),
        _ => anyhow::bail!(
            "Invalid role: {}. Valid roles: unknown, newbie, member, staff, admin",
            role_str
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_role_accepts_every_role_case_insensitively() {
        for (input, expected) in [
            ("unknown", UserRole::Unknown),
            ("NEWBIE", UserRole::Newbie),
            ("Member", UserRole::Member),
            ("sTaFf", UserRole::Staff),
            ("admin", UserRole::Admin),
        ] {
            assert_eq!(parse_role(input).unwrap(), expected, "input {input:?}");
        }
    }

    #[test]
    fn parse_role_rejects_anything_else_and_says_what_is_valid() {
        let err = parse_role("superuser").unwrap_err().to_string();
        assert!(err.contains("superuser"), "{err}");
        assert!(
            err.contains("unknown, newbie, member, staff, admin"),
            "{err}"
        );
    }

    /// The empty string is the shape a missing `--role` argument takes if one
    /// ever reaches here, and it must be refused rather than silently becoming
    /// a role.
    #[test]
    fn parse_role_rejects_the_empty_string() {
        assert!(parse_role("").is_err());
    }
}
