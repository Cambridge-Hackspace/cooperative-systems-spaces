use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, put},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    api::{
        errors::ApiError,
        responses::ApiResponse,
    },
    auth::{AuthUser, AdminUser},
    profile::{ProfileValidator, AuditLogger},
    config::ProfileField,
    models::AuditEventType,
    AppState,
};
use crate::auth::MemberUser;

pub fn profile_routes() -> Router<AppState> {
    Router::new()
        .route("/:user_id", get(get_user_profile))
        .route("/:user_id", put(update_user_profile))
        .route("/config", get(get_profile_config))
        .route("/config", put(update_profile_config))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub profile: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProfileConfigRequest {
    pub profile_fields: Vec<ProfileField>,
    pub profiles_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub user_id: Uuid,
    pub profile: Value,
}

#[derive(Debug, Serialize)]
pub struct ProfileConfigResponse {
    pub profile_fields: Vec<ProfileField>,
    pub profiles_enabled: bool,
}

/// Get user profile
async fn get_user_profile(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ProfileResponse>>, ApiError> {
    // Users can view their own profile, or staff/admin users can view any profile
    if auth_user.0.id != user_id && !auth_user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden("You can only view your own profile".to_string()));
    }

    // Get user from database
    let user = state.db.find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    let profile_response = ProfileResponse {
        user_id: user.id,
        profile: user.profile,
    };

    Ok(Json(ApiResponse::success(profile_response)))
}

/// Update user profile
async fn update_user_profile(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateProfileRequest>,
) -> Result<Json<ApiResponse<ProfileResponse>>, ApiError> {
    // Users can only update their own profile, unless they're staff/admin
    if auth_user.0.id != user_id && !auth_user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden("You can only update your own profile".to_string()));
    }

    // Check if profiles are enabled
    let config_guard = state.config_manager.get_config();
    if !config_guard.user.profiles_enabled {
        return Err(ApiError::BadRequest("User profiles are currently disabled".to_string()));
    }

    // Check if user exists
    let existing_user = state.db.find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Validate the profile data
    let profile_validator = ProfileValidator::new(&config_guard.user);
    profile_validator.validate_profile(&payload.profile)
        .map_err(|e| ApiError::BadRequest(format!("Profile validation failed: {}", e)))?;

    // Drop the config guard before database operations
    drop(config_guard);

    // Update the profile
    let updated_user = state.db.update_user_profile(user_id, &payload.profile)
        .map_err(ApiError::from)?;

    // Log the profile update
    let audit_logger = AuditLogger::new(state.db.clone());
    if let Err(e) = audit_logger.log_profile_update(
        user_id,
        if auth_user.0.id != user_id { Some(auth_user.0.id) } else { None },
        &existing_user.profile,
        &payload.profile,
        None, // We'll add IP address extraction later
        None, // We'll add user agent extraction later
    ).await {
        tracing::warn!("Failed to log profile update: {}", e);
    }

    let profile_response = ProfileResponse {
        user_id: updated_user.id,
        profile: updated_user.profile,
    };

    Ok(Json(ApiResponse::success_with_message(
        profile_response,
        "Profile updated successfully".to_string(),
    )))
}

/// Get profile configuration (admin only)
async fn get_profile_config(
    _admin_user: MemberUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ProfileConfigResponse>>, ApiError> {
    let config = state.config_manager.get_config();

    let response = ProfileConfigResponse {
        profile_fields: config.user.profile_fields.clone(),
        profiles_enabled: config.user.profiles_enabled,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// Update profile configuration (admin only)
async fn update_profile_config(
    admin_user: MemberUser,
    State(state): State<AppState>,
    Json(payload): Json<UpdateProfileConfigRequest>,
) -> Result<Json<ApiResponse<ProfileConfigResponse>>, ApiError> {
    // Validate the profile fields configuration
    for field in &payload.profile_fields {
        if field.key.is_empty() {
            return Err(ApiError::BadRequest("Profile field key cannot be empty".to_string()));
        }
        if field.label.is_empty() {
            return Err(ApiError::BadRequest("Profile field label cannot be empty".to_string()));
        }
    }

    // Check for duplicate field keys
    let mut keys = std::collections::HashSet::new();
    for field in &payload.profile_fields {
        if !keys.insert(&field.key) {
            return Err(ApiError::BadRequest(format!("Duplicate field key: {}", field.key)));
        }
    }

    // Update the configuration
    {
        let mut config = state.config_manager.get_config();
        config.user.profile_fields = payload.profile_fields.clone();
        config.user.profiles_enabled = payload.profiles_enabled;

        // Note: In a real application, you'd want to save this to the config file
        // For now, this just updates the in-memory configuration
    }

    // Log the configuration change
    let audit_logger = AuditLogger::new(state.db.clone());
    if let Err(e) = audit_logger.log_event(
        AuditEventType::AdminConfigReload,
        Some(admin_user.0.id),
        Some(admin_user.0.id),
        serde_json::json!({
            "section": "user_profiles",
            "profile_fields_count": payload.profile_fields.len(),
            "profiles_enabled": payload.profiles_enabled,
            "action": "Profile configuration updated by admin"
        }),
        None,
        None,
    ).await {
        tracing::warn!("Failed to log config update: {}", e);
    }

    let response = ProfileConfigResponse {
        profile_fields: payload.profile_fields,
        profiles_enabled: payload.profiles_enabled,
    };

    Ok(Json(ApiResponse::success_with_message(
        response,
        "Profile configuration updated successfully".to_string(),
    )))
}