use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::AdminUser,
    models::{AuditLog, UpdateUser, UserRole},
    AppState,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct RosterUser {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub full_name: String,
    pub is_active: bool,
    pub role: UserRole,
    pub created_at: chrono::NaiveDateTime,
    /// `Some(_)` when the user has at least one confirmed MFA method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: UserRole,
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/reload-config", post(reload_config))
        .route("/roster", get(get_roster))
        .route("/users/{user_id}/role", put(update_user_role))
        .route("/users/{user_id}/activate", put(activate_user))
        .route("/users/{user_id}/deactivate", put(deactivate_user))
        .route(
            "/users/{user_id}/mfa",
            axum::routing::delete(reset_user_mfa),
        )
        .route("/audit-logs", get(get_audit_logs))
        .route("/users/{user_id}/roles", post(assign_user_role))
        .route(
            "/users/{user_id}/roles/{role_id}",
            axum::routing::delete(unassign_user_role),
        )
        .nest("/rbac", crate::api::rbac_admin::admin_routes())
        .route("/pages/wiki/refresh", post(refresh_wiki_pages))
        .route("/pages/site/refresh", post(refresh_site_pages))
        .nest("/devices", crate::api::devices::admin_devices_routes())
        .nest("/webhooks", crate::api::webhooks::admin_webhook_routes())
        .nest("/doors", crate::api::doors::admin_routes())
        .nest("/places", crate::api::places::admin_routes())
        .nest("/power", crate::api::power::admin_routes())
        .nest("/groupsio", crate::api::groupsio::admin_routes())
        .nest("/membership", crate::api::membership::admin_routes())
        .nest("/tool-billing", crate::api::tool_billing::admin_routes())
        .nest("/tool-tiers", crate::api::tool_tiers::admin_routes())
        .nest("/schedules", crate::api::schedules::admin_routes())
        .nest("/home-links", crate::api::home_links::admin_routes())
}

// ---- User <-> role assignment (#65 Phase 3) --------------------------------
//
// The multi-role assignment surface. `users.role` stays the denormalized primary
// role (edited via `PUT /users/{id}/role`); these endpoints add and remove the
// *additional* roles a user holds. Authorization is the union across all of
// them, so an assignment takes effect on the user's next request.

#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role_id: Uuid,
}

/// `POST /api/admin/users/{user_id}/roles` — grant a user an additional role.
async fn assign_user_role(
    admin_user: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<AssignRoleRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("user not found".to_string()))?;
    let role = state
        .db
        .get_role(payload.role_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("role not found".to_string()))?;

    state
        .db
        .assign_user_role(user_id, payload.role_id)
        .map_err(ApiError::from)?;

    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserRoleAssigned,
            Some(user_id),
            Some(admin_user.0.id),
            serde_json::json!({ "role_id": role.id, "role": role.name }),
            None,
            None,
        )
        .await
    {
        tracing::warn!("failed to log user role assignment: {e}");
    }
    Ok(Json(ApiResponse::success(())))
}

/// `DELETE /api/admin/users/{user_id}/roles/{role_id}` — remove a role from a
/// user. Refused if it would drop the last administrator's `admin.access`.
async fn unassign_user_role(
    admin_user: AdminUser,
    State(state): State<AppState>,
    Path((user_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    // Last-admin protection: if the user is currently an effective admin and
    // removing this role would drop that, and they are the only active admin,
    // refuse -- otherwise the deployment could lock everyone out of admin.
    if state
        .db
        .user_has_permission(user_id, "admin.access")
        .map_err(ApiError::from)?
    {
        let remaining: Vec<Uuid> = state
            .db
            .user_role_ids(user_id)
            .map_err(ApiError::from)?
            .into_iter()
            .filter(|r| *r != role_id)
            .collect();
        let still_admin = state.db.rbac().has_permission(&remaining, "admin.access");
        if !still_admin && state.db.count_active_admins().map_err(ApiError::from)? <= 1 {
            return Err(ApiError::Forbidden(
                "cannot remove the last administrator's admin access".to_string(),
            ));
        }
    }

    let removed = state
        .db
        .unassign_user_role(user_id, role_id)
        .map_err(ApiError::from)?;
    if removed == 0 {
        return Err(ApiError::NotFound(
            "the user does not hold that role".to_string(),
        ));
    }

    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserRoleUnassigned,
            Some(user_id),
            Some(admin_user.0.id),
            serde_json::json!({ "role_id": role_id }),
            None,
            None,
        )
        .await
    {
        tracing::warn!("failed to log user role unassignment: {e}");
    }
    Ok(Json(ApiResponse::success(())))
}

/// Reload configuration from disk (admin only)
async fn reload_config(
    _admin_user: AdminUser, // Ensures only admin users can access
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    // Attempt to reload the configuration
    match state.config_manager.reload_config() {
        Ok(()) => {
            let new_config = state.config_manager.get_config();
            Ok(Json(ApiResponse::success_with_message(
                serde_json::json!({
                    "site_name": new_config.site.site_name,
                    "debug_mode": new_config.site.debug,
                    "initial_setup_enabled": new_config.initial_setup.setup_enabled,
                    "auth_config": {
                        "allow_registration": new_config.auth.allow_registration,
                        "require_email_verification": new_config.auth.require_email_verification,
                        "password_min_length": new_config.auth.password_min_length
                    }
                }),
                "Configuration reloaded successfully".to_string(),
            )))
        }
        Err(e) => {
            //  rather than : anyhow's alternate format prints the
            // whole cause chain, and a config reload fails for a reason three
            // layers down more often than not.
            tracing::error!("Failed to reload configuration: {:#}", e);
            Err(ApiError::InternalServerError(format!(
                "Failed to reload configuration: {:#}",
                e
            )))
        }
    }
}

/// Get all users for roster management (admin only)
async fn get_roster(
    _admin_user: AdminUser, // Ensures only admin users can access
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<RosterUser>>>, ApiError> {
    let users = state
        .db
        .get_all_users()
        .map_err(|e| ApiError::from_db("Failed to query users", e))?;

    let roster_users: Vec<RosterUser> = users
        .into_iter()
        .map(|user| RosterUser {
            id: user.id,
            username: user.username,
            email: user.email,
            full_name: user.full_name,
            is_active: user.is_active,
            role: user.role,
            created_at: user.created_at,
            mfa_enrolled_at: user.mfa_enrolled_at,
        })
        .collect();

    Ok(Json(ApiResponse::success(roster_users)))
}

/// Update a user's role (admin only)
async fn update_user_role(
    _admin_user: AdminUser, // Ensures only admin users can access
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateUserRoleRequest>,
) -> Result<Json<ApiResponse<RosterUser>>, ApiError> {
    // First check if user exists
    let user = state
        .db
        .find_user_by_id(user_id)
        .map_err(|e| ApiError::from_db("Failed to check if user exists", e))?;

    let _user = user.ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Update the user's role
    let update_data = UpdateUser {
        username: None,
        email: None,
        password_hash: None,
        full_name: None,
        is_active: None,
        role: Some(payload.role),
        profile: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
        meta: None,
    };

    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(|e| ApiError::from_db("Failed to update user role", e))?;

    let roster_user = RosterUser {
        id: updated_user.id,
        username: updated_user.username.clone(),
        email: updated_user.email.clone(),
        full_name: updated_user.full_name,
        is_active: updated_user.is_active,
        role: updated_user.role.clone(),
        created_at: updated_user.created_at,
        mfa_enrolled_at: updated_user.mfa_enrolled_at,
    };

    // Log the role change
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserRoleChange,
            Some(updated_user.id),
            Some(_admin_user.0.id),
            serde_json::json!({
                "old_role": "unknown", // We don't have the old role easily accessible
                "new_role": format!("{:?}", updated_user.role),
                "username": updated_user.username,
                "action": "User role updated by admin"
            }),
            None,
            None,
        )
        .await
    {
        tracing::warn!("Failed to log role change: {}", e);
    }

    // Role change may affect door allow-lists (rules of kind=role).
    state.door_service.republish_all();

    Ok(Json(ApiResponse::success_with_message(
        roster_user,
        "User role updated successfully".to_string(),
    )))
}

/// Activate a user (admin only)
async fn activate_user(
    _admin_user: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<RosterUser>>, ApiError> {
    let user = state
        .db
        .find_user_by_id(user_id)
        .map_err(|e| ApiError::from_db("Failed to check if user exists", e))?;

    let _user = user.ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Update the user's active status
    let update_data = UpdateUser {
        username: None,
        email: None,
        password_hash: None,
        full_name: None,
        is_active: Some(true),
        role: None,
        profile: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
        meta: None,
    };

    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(|e| ApiError::from_db("Failed to activate user", e))?;

    let roster_user = RosterUser {
        id: updated_user.id,
        username: updated_user.username.clone(),
        email: updated_user.email.clone(),
        full_name: updated_user.full_name,
        is_active: updated_user.is_active,
        role: updated_user.role,
        created_at: updated_user.created_at,
        mfa_enrolled_at: updated_user.mfa_enrolled_at,
    };

    // Log the activation
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserActivation,
            Some(updated_user.id),
            Some(_admin_user.0.id),
            serde_json::json!({
                "username": updated_user.username,
                "action": "User activated by admin"
            }),
            None,
            None,
        )
        .await
    {
        tracing::warn!("Failed to log user activation: {}", e);
    }

    // Activation may pull this user into role-based door allow-lists.
    state.door_service.republish_all();

    Ok(Json(ApiResponse::success_with_message(
        roster_user,
        "User activated successfully".to_string(),
    )))
}

/// Deactivate a user (admin only)
async fn deactivate_user(
    _admin_user: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<RosterUser>>, ApiError> {
    let user = state
        .db
        .find_user_by_id(user_id)
        .map_err(|e| ApiError::from_db("Failed to check if user exists", e))?;

    let _user = user.ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Update the user's active status
    let update_data = UpdateUser {
        username: None,
        email: None,
        password_hash: None,
        full_name: None,
        is_active: Some(false),
        role: None,
        profile: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
        meta: None,
    };

    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(|e| ApiError::from_db("Failed to deactivate user", e))?;

    let roster_user = RosterUser {
        id: updated_user.id,
        username: updated_user.username.clone(),
        email: updated_user.email.clone(),
        full_name: updated_user.full_name,
        is_active: updated_user.is_active,
        role: updated_user.role,
        created_at: updated_user.created_at,
        mfa_enrolled_at: updated_user.mfa_enrolled_at,
    };

    // Log the deactivation
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserDeactivation,
            Some(updated_user.id),
            Some(_admin_user.0.id),
            serde_json::json!({
                "username": updated_user.username,
                "action": "User deactivated by admin"
            }),
            None,
            None,
        )
        .await
    {
        tracing::warn!("Failed to log user deactivation: {}", e);
    }

    // Deactivation should drop this user from role-based door allow-lists.
    state.door_service.republish_all();

    Ok(Json(ApiResponse::success_with_message(
        roster_user,
        "User deactivated successfully".to_string(),
    )))
}

#[derive(Debug, Deserialize, Default)]
pub struct AuditLogQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub event_type: Option<String>,
}

/// Get audit logs (admin only)
async fn get_audit_logs(
    _admin_user: AdminUser, // Ensures only admin users can access
    State(state): State<AppState>,
    query: Query<AuditLogQuery>,
) -> Result<Json<ApiResponse<Vec<AuditLog>>>, ApiError> {
    let page = query.page.unwrap_or(1);
    let per_page = std::cmp::min(query.per_page.unwrap_or(50), 100); // Cap at 100 records per page
    let offset = (page - 1) * per_page;

    let logs = state
        .db
        .get_audit_logs(offset as i64, per_page as i64, query.event_type.clone())
        .map_err(|e| ApiError::from_db("Failed to query audit logs", e))?;

    Ok(Json(ApiResponse::success(logs)))
}

/// Refresh wiki pages from repository (admin only)
async fn refresh_wiki_pages(
    _admin_user: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    // Asked before it is attempted, rather than inferred from the failure.
    //
    // "No repository configured" is a state an administrator put the instance
    // in, not a fault -- so a 500 tells them the server broke about a setting
    // they can change in the next screen. 409 says what is actually true: the
    // request conflicts with the current configuration.
    //
    // Checked against the config rather than by matching the error's text. The
    // message is ours today and one refactor away from being somebody else's,
    // and a status code that depends on a string is a status code that changes
    // when a message is reworded.
    if state.config_manager.get_config().pages.wiki_repo.is_none() {
        return Err(ApiError::Conflict(
            "No wiki repository is configured; set one before refreshing".to_string(),
        ));
    }

    let mut pages_service = state.pages_service.write().await;

    match pages_service.trigger_wiki_update().await {
        Ok(()) => {
            let store = pages_service.get_store();
            Ok(Json(ApiResponse::success_with_message(
                serde_json::json!({
                    "wiki_pages_count": store.wiki_pages.len(),
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                }),
                format!(
                    "Wiki pages refreshed successfully. {} pages loaded.",
                    store.wiki_pages.len()
                ),
            )))
        }
        Err(e) => {
            tracing::error!("Failed to refresh wiki pages: {}", e);
            Err(ApiError::InternalServerError(format!(
                "Failed to refresh wiki pages: {}",
                e
            )))
        }
    }
}

/// Refresh site pages from repository (admin only)
async fn refresh_site_pages(
    _admin_user: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    // Asked before it is attempted, rather than inferred from the failure.
    //
    // "No repository configured" is a state an administrator put the instance
    // in, not a fault -- so a 500 tells them the server broke about a setting
    // they can change in the next screen. 409 says what is actually true: the
    // request conflicts with the current configuration.
    //
    // Checked against the config rather than by matching the error's text. The
    // message is ours today and one refactor away from being somebody else's,
    // and a status code that depends on a string is a status code that changes
    // when a message is reworded.
    if state.config_manager.get_config().pages.site_repo.is_none() {
        return Err(ApiError::Conflict(
            "No site repository is configured; set one before refreshing".to_string(),
        ));
    }

    let mut pages_service = state.pages_service.write().await;

    match pages_service.trigger_site_update().await {
        Ok(()) => {
            let store = pages_service.get_store();
            Ok(Json(ApiResponse::success_with_message(
                serde_json::json!({
                    "site_pages_count": store.site_pages.len(),
                    "has_index": store.site_index.is_some(),
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                }),
                format!(
                    "Site pages refreshed successfully. {} pages loaded.",
                    store.site_pages.len()
                ),
            )))
        }
        Err(e) => {
            tracing::error!("Failed to refresh site pages: {}", e);
            Err(ApiError::InternalServerError(format!(
                "Failed to refresh site pages: {}",
                e
            )))
        }
    }
}

/// DELETE /api/admin/users/{user_id}/mfa — wipe every MFA artifact for a user.
/// Used for lockout recovery; the user will be able to log in with just a
/// password until they re-enroll. Always audited.
async fn reset_user_mfa(
    admin_user: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let target = state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    state.db.reset_user_mfa(user_id).map_err(ApiError::from)?;

    // Emit one event per disabled artifact category so webhook subscribers
    // can see exactly what was reset, plus an aggregate audit entry.
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::MfaTotpDisabled,
            Some(user_id),
            Some(admin_user.0.id),
            serde_json::json!({ "reason": "admin_reset", "target_username": target.username }),
            None,
            None,
        )
        .await
    {
        tracing::warn!("Failed to log MFA reset audit: {}", e);
    }

    Ok(Json(ApiResponse::success_with_message(
        serde_json::json!({ "user_id": user_id }),
        format!("MFA reset for user {}", target.username),
    )))
}
