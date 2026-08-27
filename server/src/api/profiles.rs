use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{get, post, put},
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
    models::{AuditEventType, ProfileConfigVersion},
    AppState,
};

pub fn profile_routes() -> Router<AppState> {
    Router::new()
        .route("/{user_id}", get(get_user_profile))
        .route("/{user_id}", put(update_user_profile))
        .route("/config", get(get_profile_config))
        .route("/config", put(update_profile_config))
        .route("/config/versions", get(list_profile_config_versions))
        .route("/config/rollback/{version}", post(rollback_profile_config))
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

#[derive(Debug, Serialize)]
pub struct ProfileConfigVersionResponse {
    pub version: i64,
    pub profile_fields: Vec<ProfileField>,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl TryFrom<ProfileConfigVersion> for ProfileConfigVersionResponse {
    type Error = serde_json::Error;

    fn try_from(v: ProfileConfigVersion) -> Result<Self, Self::Error> {
        Ok(Self {
            version: v.version,
            profile_fields: serde_json::from_value(v.profile_fields)?,
            created_by: v.created_by,
            created_at: v.created_at,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct VersionListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
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

    // If the door-relevant profile field changed (it's reused by the door
    // module via `toolguard.profile_field`), republish state to every edge
    // device serving doors so allow-lists pick up the new card(s).
    let card_field = state.config_manager.get_config().toolguard.profile_field.clone();
    let old_card = existing_user.profile.get(&card_field).cloned();
    let new_card = payload.profile.get(&card_field).cloned();
    if old_card != new_card {
        state.door_service.republish_all();
    }

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

/// Get profile configuration (any authenticated user — needed so every
/// user's own profile page can tell whether profiles are enabled and which
/// fields to render; only `update_profile_config` is admin-restricted).
async fn get_profile_config(
    _auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ProfileConfigResponse>>, ApiError> {
    // profile_fields and profiles_enabled are versioned together in the
    // database, which is authoritative across instances.
    let profile_fields = current_profile_fields(&state).await?;
    let profiles_enabled = state.config_manager.get_config().user.profiles_enabled;

    let response = ProfileConfigResponse {
        profile_fields,
        profiles_enabled,
    };

    Ok(Json(ApiResponse::success(response)))
}

/// Update profile configuration (admin only)
async fn update_profile_config(
    admin_user: AdminUser,
    State(state): State<AppState>,
    Json(payload): Json<UpdateProfileConfigRequest>,
) -> Result<Json<ApiResponse<ProfileConfigResponse>>, ApiError> {
    validate_profile_fields(&payload.profile_fields)?;

    // Persist a new, immutable version of the field schema and enabled
    // toggle together...
    let fields_json = serde_json::to_value(&payload.profile_fields)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to serialize profile fields: {}", e)))?;
    let new_version = state.db
        .insert_profile_config_version(fields_json, payload.profiles_enabled, Some(admin_user.0.id))
        .map_err(ApiError::from)?;

    // ...and keep the in-process config cache (used by validation) in sync.
    state.config_manager.set_profile_fields(payload.profile_fields.clone());
    state.config_manager.set_profiles_enabled(payload.profiles_enabled);

    // Log the configuration change
    let audit_logger = AuditLogger::new(state.db.clone());
    if let Err(e) = audit_logger.log_event(
        AuditEventType::AdminConfigReload,
        Some(admin_user.0.id),
        Some(admin_user.0.id),
        serde_json::json!({
            "section": "user_profiles",
            "profile_config_version": new_version.version,
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

/// List profile field schema version history, newest first (admin only)
async fn list_profile_config_versions(
    _admin_user: AdminUser,
    State(state): State<AppState>,
    Query(q): Query<VersionListQuery>,
) -> Result<Json<ApiResponse<Vec<ProfileConfigVersionResponse>>>, ApiError> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);

    let versions = state.db.list_profile_config_versions(limit, offset)
        .map_err(ApiError::from)?
        .into_iter()
        .map(ProfileConfigVersionResponse::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::InternalServerError(format!("Failed to decode stored profile fields: {}", e)))?;

    Ok(Json(ApiResponse::success(versions)))
}

/// Roll back to a prior field schema version by inserting a new version
/// carrying that version's `profile_fields` (admin only). History is
/// never mutated or deleted, so this itself shows up as a new entry.
async fn rollback_profile_config(
    admin_user: AdminUser,
    State(state): State<AppState>,
    Path(target_version): Path<i64>,
) -> Result<Json<ApiResponse<ProfileConfigResponse>>, ApiError> {
    let target = state.db.get_profile_config_version(target_version)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound(format!("No profile config version {}", target_version)))?;

    let profile_fields: Vec<ProfileField> = serde_json::from_value(target.profile_fields.clone())
        .map_err(|e| ApiError::InternalServerError(format!("Failed to decode stored profile fields: {}", e)))?;

    let new_version = state.db
        .insert_profile_config_version(target.profile_fields, target.profiles_enabled, Some(admin_user.0.id))
        .map_err(ApiError::from)?;

    state.config_manager.set_profile_fields(profile_fields.clone());
    state.config_manager.set_profiles_enabled(target.profiles_enabled);

    let audit_logger = AuditLogger::new(state.db.clone());
    if let Err(e) = audit_logger.log_event(
        AuditEventType::AdminConfigReload,
        Some(admin_user.0.id),
        Some(admin_user.0.id),
        serde_json::json!({
            "section": "user_profiles",
            "profile_config_version": new_version.version,
            "rolled_back_to": target_version,
            "action": "Profile configuration rolled back by admin"
        }),
        None,
        None,
    ).await {
        tracing::warn!("Failed to log config rollback: {}", e);
    }

    let profiles_enabled = state.config_manager.get_config().user.profiles_enabled;
    let response = ProfileConfigResponse {
        profile_fields,
        profiles_enabled,
    };

    Ok(Json(ApiResponse::success_with_message(
        response,
        format!("Rolled back to profile configuration version {}", target_version),
    )))
}

/// Validate a submitted set of profile field definitions: no empty keys or
/// labels, and no duplicate keys.
fn validate_profile_fields(fields: &[ProfileField]) -> Result<(), ApiError> {
    for field in fields {
        if field.key.is_empty() {
            return Err(ApiError::BadRequest("Profile field key cannot be empty".to_string()));
        }
        if field.label.is_empty() {
            return Err(ApiError::BadRequest("Profile field label cannot be empty".to_string()));
        }
    }

    let mut keys = std::collections::HashSet::new();
    for field in fields {
        if !keys.insert(&field.key) {
            return Err(ApiError::BadRequest(format!("Duplicate field key: {}", field.key)));
        }
    }

    Ok(())
}

/// The current profile field schema, read from the database (authoritative
/// across instances) and falling back to the in-process config cache only
/// if no version has ever been saved (shouldn't happen once main.rs's
/// startup bootstrap has run, but keeps this handler standalone).
async fn current_profile_fields(state: &AppState) -> Result<Vec<ProfileField>, ApiError> {
    match state.db.get_latest_profile_config_version().map_err(ApiError::from)? {
        Some(latest) => serde_json::from_value(latest.profile_fields)
            .map_err(|e| ApiError::InternalServerError(format!("Failed to decode stored profile fields: {}", e))),
        None => Ok(state.config_manager.get_config().user.profile_fields.clone()),
    }
}