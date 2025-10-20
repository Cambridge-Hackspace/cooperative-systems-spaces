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
    pub tool_id: String,      // Tool ID
    #[serde(default)]
    pub api_key: Option<String>, // Optional API key for authentication
}

/// Request parameters for tool logging
#[derive(Debug, Deserialize)]
pub struct ToolLogRequest {
    pub card: String,
    pub tool_id: String,
    pub seconds: f32,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub api_key: Option<String>, // Optional API key for authentication
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
    // Validate API key (no specific tool context for user management)
    if !validate_api_key(&state, &req.api_key, None).await? {
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
    // Validate API key (no specific tool context for user management)
    if !validate_api_key(&state, &req.api_key, None).await? {
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
            log_tool_access_denied(&state, None, &req.tool_id, "Unknown card").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Unknown card")));
        }
    };

    // Check if user is active
    if !user.is_active {
        log_tool_access_denied(&state, Some(&user), &req.tool_id, "User is not active").await?;
        return Ok(Json(ToolPassResponse::tool_denied("User is not active")));
    }

    // Find tool by ID or external_id
    let tool = match find_tool_by_toolpass_id(&state, &req.tool_id).await? {
        Some(tool) => tool,
        None => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool not found").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool not found")));
        }
    };

    // Check if tool is available - must be in Idle status
    // Reject if tool is in any state other than Idle (InUse, Maintenance, Broken, Repair, Retired)
    match tool.status {
        crate::models::ToolStatus::Idle => {
            // Tool is available, continue with checks
        }
        crate::models::ToolStatus::InUse => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is already in use").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool is already in use")));
        }
        crate::models::ToolStatus::Maintenance => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is under maintenance").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool is under maintenance")));
        }
        crate::models::ToolStatus::Broken => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is broken").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool is broken")));
        }
        crate::models::ToolStatus::Repair => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is in repair").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool is in repair")));
        }
        crate::models::ToolStatus::Retired => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is retired").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Tool is retired")));
        }
    }

    // Check if tool has training steps defined
    let has_training_steps = state.db.tool_has_training_steps(tool.id)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to check training steps: {}", e)))?;
    
    // If training steps exist, check if user has completed all of them
    if has_training_steps {
        let has_completed_training = state.db.user_has_completed_all_training_steps(user.id, tool.id)
            .map_err(|e| ApiError::InternalServerError(format!("Failed to check training completion: {}", e)))?;

        if !has_completed_training {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Training required").await?;
            return Ok(Json(ToolPassResponse::tool_denied("Training required")));
        }
    }
    // If no training steps are defined, allow access

    // Update tool status to InUse
    state.db.update_tool_status(tool.id, &crate::models::ToolStatus::InUse)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to update tool status: {}", e)))?;

    // Create tool event for activation
    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "activated".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: Some(crate::models::ToolStatus::InUse),
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!("Activated via ToolPass (tool_id: {})", req.tool_id)),
        scan_data: Some(serde_json::json!({
            "toolpass_id": req.tool_id,
            "card": req.card,
        })),
    };
    
    state.db.create_tool_event(&event)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;

    // Log the tool activation
    log_tool_activated(&state, &user, tool.id, &req.tool_id).await?;

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

    // Find tool by ID or external_id
    let tool = match find_tool_by_toolpass_id(&state, &req.tool_id).await? {
        Some(tool) => tool,
        None => {
            return Ok(Json(ToolPassResponse::error("Tool not found")));
        }
    };

    // Update tool status to Idle
    state.db.update_tool_status(tool.id, &crate::models::ToolStatus::Idle)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to update tool status: {}", e)))?;

    // Create tool event for deactivation
    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "deactivated".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: Some(crate::models::ToolStatus::Idle),
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!("Deactivated via ToolPass (tool_id: {})", req.tool_id)),
        scan_data: Some(serde_json::json!({
            "toolpass_id": req.tool_id,
            "card": req.card,
        })),
    };
    
    state.db.create_tool_event(&event)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;

    // Log the tool deactivation
    log_tool_deactivated(&state, &user, tool.id, &req.tool_id).await?;

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

    // Find tool by ID or external_id
    let tool = match find_tool_by_toolpass_id(&state, &req.tool_id).await? {
        Some(tool) => tool,
        None => {
            return Ok(Json(ToolPassResponse::error("Tool not found")));
        }
    };

    // Create tool event for usage logging
    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "usage_logged".to_string(),
        old_status: None,
        new_status: None,
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!("Usage logged: {:.1} minutes", req.seconds / 60.0)),
        scan_data: Some(serde_json::json!({
            "toolpass_id": req.tool_id,
            "card": req.card,
            "seconds": req.seconds,
            "temperature": req.temperature,
        })),
    };
    
    state.db.create_tool_event(&event)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;

    // Log the tool usage
    log_tool_usage(&state, &user, tool.id, &req.tool_id, req.seconds, req.temperature).await?;

    Ok(Json(ToolPassResponse::ok_with_message("Usage logged")))
}

/// Helper: Validate API key
/// Checks against the global API key from config OR the tool-specific external_api_key
/// Returns true only if a valid key is provided - no unauthenticated access allowed
async fn validate_api_key(state: &AppState, api_key: &str, tool: Option<&crate::models::Tool>) -> Result<bool, ApiError> {
    let config = state.config_manager.get_config();
    
    // Reject empty API keys immediately
    if api_key.is_empty() {
        return Ok(false);
    }
    
    // First, check if the tool has a specific API key
    if let Some(tool) = tool {
        if let Some(tool_api_key) = &tool.external_api_key {
            if !tool_api_key.is_empty() && tool_api_key == api_key {
                return Ok(true);
            }
        }
    }
    
    // Fall back to global API key
    if let Some(global_key) = &config.toolpass.global_api_key {
        if !global_key.is_empty() && global_key == api_key {
            return Ok(true);
        }
    }
    
    // No valid API key found - deny access
    Ok(false)
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

/// Helper: Find tool by ToolPass ID
/// First tries to find by external_id matching the toolpass_id, then falls back to index-based lookup
async fn find_tool_by_toolpass_id(state: &AppState, toolpass_id: &str) -> Result<Option<crate::models::Tool>, ApiError> {
    // First, try to find by external_id (if the tool has one set)
    let external_id_str = toolpass_id.to_string();
    if let Ok(Some(tool)) = state.db.get_tool_by_external_id(&external_id_str) {
        return Ok(Some(tool));
    }
    
    Ok(None)
}

/// Helper: Log access denied event
async fn log_tool_access_denied(
    state: &AppState,
    user: Option<&crate::models::User>,
    toolpass_id: &str,
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
    toolpass_id: &str,
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
    toolpass_id: &str,
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
    toolpass_id: &str,
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
