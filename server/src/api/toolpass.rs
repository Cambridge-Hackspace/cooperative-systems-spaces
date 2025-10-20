use axum::{
    extract::{Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::tools::ToolQuery;
use crate::AppState;

/// Standard ToolPass API response
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolPassResponse {
    pub api_version: f32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_off: Option<bool>,
}

impl ToolPassResponse {
    fn ok() -> Self {
        Self {
            api_version: 1.0,
            status: "ok".to_string(),
            message: None,
            tool_on: None,
            tool_off: None,
        }
    }

    fn ok_with_message(message: impl Into<String>) -> Self {
        Self {
            api_version: 1.0,
            status: "ok".to_string(),
            message: Some(message.into()),
            tool_on: None,
            tool_off: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            api_version: 1.0,
            status: "error".to_string(),
            message: Some(message.into()),
            tool_on: None,
            tool_off: None,
        }
    }

    fn tool_authorized() -> Self {
        Self {
            api_version: 1.0,
            status: "ok".to_string(),
            message: Some("Tool authorized".to_string()),
            tool_on: Some(true),
            tool_off: None,
        }
    }

    fn tool_denied(reason: impl Into<String>) -> Self {
        Self {
            api_version: 1.0,
            status: "error".to_string(),
            message: Some(reason.into()),
            tool_on: Some(false),
            tool_off: None,
        }
    }

    fn tool_off_ok() -> Self {
        Self {
            api_version: 1.0,
            status: "ok".to_string(),
            message: Some("Tool deactivated".to_string()),
            tool_on: None,
            tool_off: Some(true),
        }
    }
}

/// Request parameters for adding a user
#[derive(Debug, Deserialize)]
pub struct AddUserRequest {
    pub api_key: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
}

/// Request parameters for removing a user
#[derive(Debug, Deserialize)]
pub struct RemoveUserRequest {
    pub api_key: String,
    pub email: String,
}

/// Request parameters for tool operations
#[derive(Debug, Deserialize)]
pub struct ToolRequest {
    pub card: String,      // RFID card ID or user identifier
    pub tool_id: i32,      // Tool ID
}

/// Request parameters for tool logging
#[derive(Debug, Deserialize)]
pub struct ToolLogRequest {
    pub card: String,
    pub tool_id: i32,
    pub seconds: f32,
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// Configure ToolPass API routes
pub fn toolpass_routes() -> Router<AppState> {
    Router::new()
        .route("/v1", get(api_status))
        .route("/v1/add-user", post(add_user))
        .route("/v1/remove-user", post(remove_user))
        .route("/v1/tool-on", get(tool_on))
        .route("/v1/tool-off", get(tool_off))
        .route("/v1/tool-log", get(tool_log))
}

/// GET /api/v1 - API status check
async fn api_status() -> Json<ToolPassResponse> {
    Json(ToolPassResponse::ok())
}

/// POST /api/v1/add-user - Add a new member
async fn add_user(
    State(state): State<AppState>,
    Json(req): Json<AddUserRequest>,
) -> Result<Json<ToolPassResponse>, ApiError> {
    // Validate API key
    if !validate_api_key(&state, &req.api_key).await? {
        return Ok(Json(ToolPassResponse::error("Invalid API key")));
    }

    // Check if user already exists
    if let Ok(Some(_)) = state.db.find_user_by_email(&req.email) {
        return Ok(Json(ToolPassResponse::error("User already exists")));
    }

    // Create new user
    use crate::auth::PasswordHashUtil;
    use crate::models::NewUser;

    // Generate a random password for API-created users
    // In production, you might want to send this via email or have a different flow
    let temp_password = uuid::Uuid::new_v4().to_string();
    let password_hash = PasswordHashUtil::hash(&temp_password)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to hash password: {}", e)))?;

    let full_name = format!("{} {}", req.first_name, req.last_name);
    let username = req.email.split('@').next().unwrap_or(&req.email).to_string();

    let new_user = NewUser::new(username, req.email, password_hash, full_name);

    state.db.create_user(&new_user)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create user: {}", e)))?;

    Ok(Json(ToolPassResponse::ok_with_message("User added successfully")))
}

/// POST /api/v1/remove-user - Remove a member
async fn remove_user(
    State(state): State<AppState>,
    Json(req): Json<RemoveUserRequest>,
) -> Result<Json<ToolPassResponse>, ApiError> {
    // Validate API key
    if !validate_api_key(&state, &req.api_key).await? {
        return Ok(Json(ToolPassResponse::error("Invalid API key")));
    }

    // Find user by email
    let user = state.db.find_user_by_email(&req.email)
        .map_err(|e| ApiError::InternalServerError(format!("Database error: {}", e)))?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Deactivate user instead of deleting
    state.db.deactivate_user(user.id)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to deactivate user: {}", e)))?;

    Ok(Json(ToolPassResponse::ok_with_message("User removed successfully")))
}

/// GET /api/v1/tool-on - Check authorization and activate tool
async fn tool_on(
    State(state): State<AppState>,
    Query(req): Query<ToolRequest>,
) -> Result<Json<ToolPassResponse>, ApiError> {
    tracing::info!("Tool on request: card={}, tool_id={}", req.card, req.tool_id);

    // Find user by card ID
    let user = match find_user_by_card(&state, &req.card).await? {
        Some(user) => user,
        None => {
            // Log denied access attempt
            log_tool_access_denied(&state, None, req.tool_id, "Unknown card").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Unknown card")));
        }
    };

    // Check if user is active
    if !user.is_active {
        log_tool_access_denied(&state, Some(&user), req.tool_id, "User is not active").await?;
        return Ok(Json(ToolPassResponse::tool_denied("User is not active")));
    }

    // Find tool by ID
    // Note: ToolPass uses integer tool_id, but we use UUID
    // We'll need to either add an integer ID field or use a mapping
    let query = ToolQuery {
        category: None,
        status: None,
        requires_training: None,
        page: None,
        per_page: None,
    };
    let tools = state.db.get_tools(query)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to list tools: {}", e)))?;

    let tool = tools.iter()
        .enumerate()
        .find(|(idx, _)| *idx as i32 == req.tool_id)
        .map(|(_, tool)| tool);

    let tool = match tool {
        Some(tool) => tool,
        None => {
            log_tool_access_denied(&state, Some(&user), req.tool_id, "Tool not found").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool not found")));
        }
    };

    // Check if tool requires training
    if tool.requires_training {
        let has_training = state.db.user_has_valid_training(user.id, tool.id)
            .map_err(|e| ApiError::InternalServerError(format!("Failed to check training: {}", e)))?;

        if !has_training {
            log_tool_access_denied(&state, Some(&user), req.tool_id, "Training required").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Training required")));
        }
    }

    // Log the tool activation
    log_tool_activated(&state, &user, tool.id, req.tool_id).await?;

    Ok(Json(ToolPassResponse::tool_authorized()))
}

/// GET /api/v1/tool-off - Deactivate tool
async fn tool_off(
    State(state): State<AppState>,
    Query(req): Query<ToolRequest>,
) -> Result<Json<ToolPassResponse>, ApiError> {
    tracing::info!("Tool off request: card={}, tool_id={}", req.card, req.tool_id);

    // Find user by card ID
    let user = match find_user_by_card(&state, &req.card).await? {
        Some(user) => user,
        None => {
            return Ok(Json(ToolPassResponse::error("Unknown card")));
        }
    };

    // Find tool (same logic as tool_on)
    let query = ToolQuery {
        category: None,
        status: None,
        requires_training: None,
        page: None,
        per_page: None,
    };
    let tools = state.db.get_tools(query)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to list tools: {}", e)))?;

    let tool = tools.iter()
        .enumerate()
        .find(|(idx, _)| *idx as i32 == req.tool_id)
        .map(|(_, tool)| tool);

    if let Some(tool) = tool {
        log_tool_deactivated(&state, &user, tool.id, req.tool_id).await?;
    }

    Ok(Json(ToolPassResponse::tool_off_ok()))
}

/// GET /api/v1/tool-log - Log tool usage
async fn tool_log(
    State(state): State<AppState>,
    Query(req): Query<ToolLogRequest>,
) -> Result<Json<ToolPassResponse>, ApiError> {
    tracing::info!(
        "Tool log request: card={}, tool_id={}, seconds={}, temp={:?}",
        req.card, req.tool_id, req.seconds, req.temperature
    );

    // Find user by card ID
    let user = match find_user_by_card(&state, &req.card).await? {
        Some(user) => user,
        None => {
            return Ok(Json(ToolPassResponse::error("Unknown card")));
        }
    };

    // Find tool
    let query = ToolQuery {
        category: None,
        status: None,
        requires_training: None,
        page: None,
        per_page: None,
    };
    let tools = state.db.get_tools(query)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to list tools: {}", e)))?;

    let tool = tools.iter()
        .enumerate()
        .find(|(idx, _)| *idx as i32 == req.tool_id)
        .map(|(_, tool)| tool);

    if let Some(tool) = tool {
        log_tool_usage(&state, &user, tool.id, req.tool_id, req.seconds, req.temperature).await?;
    }

    Ok(Json(ToolPassResponse::ok_with_message("Usage logged")))
}

/// Helper: Validate API key
async fn validate_api_key(state: &AppState, api_key: &str) -> Result<bool, ApiError> {
    // TODO: Implement proper API key validation
    // For now, we'll check against a configured key in the config
    let _config = state.config_manager.get_config();
    
    // Check if API key matches configured key (you'll need to add this to your config)
    // For now, accept any non-empty key as valid
    Ok(!api_key.is_empty())
}

/// Helper: Find user by card ID
/// Looks up users by the configured profile field (default: "card_id")
async fn find_user_by_card(state: &AppState, card: &str) -> Result<Option<crate::models::User>, ApiError> {
    // Get the configured profile field name for card_id
    let config = state.config_manager.get_config();
    let profile_field = &config.toolpass.profile_field;

    // Use optimized single-query database lookup
    // This uses PostgreSQL's JSONB operators to query the profile field directly
    state.db.find_user_by_profile_field(profile_field, card)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to query user by profile field: {}", e)))
}

/// Helper: Log access denied event
async fn log_tool_access_denied(
    state: &AppState,
    user: Option<&crate::models::User>,
    toolpass_id: i32,
    reason: &str,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "toolpass_id": toolpass_id,
        "reason": reason,
        "card_provided": user.is_none(),
    });

    let audit_logger = state.audit_logger.clone();
    let user_id = user.map(|u| u.id);
    
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolAccessDenied,
            user_id,
            user_id,
            details,
            None,
            None,
        ).await;
    });

    Ok(())
}

/// Helper: Log tool activated event
async fn log_tool_activated(
    state: &AppState,
    user: &crate::models::User,
    tool_id: Uuid,
    toolpass_id: i32,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "tool_id": tool_id,
        "toolpass_id": toolpass_id,
        "action": "activated",
    });

    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolActivated,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        ).await;
    });

    Ok(())
}

/// Helper: Log tool deactivated event
async fn log_tool_deactivated(
    state: &AppState,
    user: &crate::models::User,
    tool_id: Uuid,
    toolpass_id: i32,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "tool_id": tool_id,
        "toolpass_id": toolpass_id,
        "action": "deactivated",
    });

    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolDeactivated,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        ).await;
    });

    Ok(())
}

/// Helper: Log tool usage event
async fn log_tool_usage(
    state: &AppState,
    user: &crate::models::User,
    tool_id: Uuid,
    toolpass_id: i32,
    seconds: f32,
    temperature: Option<f32>,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "tool_id": tool_id,
        "toolpass_id": toolpass_id,
        "seconds": seconds,
        "temperature": temperature,
        "duration_minutes": seconds / 60.0,
    });

    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolUsageLogged,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        ).await;
    });

    Ok(())
}
