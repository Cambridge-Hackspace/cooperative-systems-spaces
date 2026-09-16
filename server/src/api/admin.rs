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
    models::{AuditLog, UpdateUser},
    pages::{PageType, PagesService},
    AppState,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct RosterUser {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub full_name: String,
    pub is_active: bool,
    pub role: String,
    pub created_at: chrono::NaiveDateTime,
    /// `Some(_)` when the user has at least one confirmed MFA method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
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
        .route(
            "/users/{user_id}/roles",
            post(assign_user_role).get(list_user_roles),
        )
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
        .nest(
            "/tool-modules",
            crate::api::tool_modules::admin_module_routes(),
        )
        .nest(
            "/tool-interlocks",
            crate::api::tool_modules::admin_interlock_routes(),
        )
        .nest("/tool-tiers", crate::api::tool_tiers::admin_routes())
        .nest("/schedules", crate::api::schedules::admin_routes())
        .nest("/home-links", crate::api::home_links::admin_routes())
}

// ---- User <-> role assignment (#65 Phase 3) --------------------------------
//
// The multi-role assignment surface. The primary tier role is edited via
// `PUT /users/{id}/role` (which rewrites the user's tier assignment in
// `user_roles` -- the `users.role` enum column is retired); these endpoints add
// and remove the *additional* roles a user holds. Authorization is the union
// across all of them, so an assignment takes effect on the user's next request.

#[derive(Debug, Deserialize)]
pub struct AssignRoleRequest {
    pub role_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct AssignedRole {
    pub id: Uuid,
    pub name: String,
}

/// `GET /api/admin/users/{user_id}/roles` — the roles a user currently holds.
async fn list_user_roles(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<AssignedRole>>>, ApiError> {
    state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("user not found".to_string()))?;
    let roles = state
        .db
        .user_assigned_roles(user_id)
        .map_err(ApiError::from)?
        .into_iter()
        .map(|(id, name)| AssignedRole { id, name })
        .collect();
    Ok(Json(ApiResponse::success(roles)))
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
        .map(|user| {
            let role = state
                .db
                .user_primary_role(user.id)
                .map_err(ApiError::from)?;
            Ok::<_, ApiError>(RosterUser {
                id: user.id,
                username: user.username,
                email: user.email,
                full_name: user.full_name,
                is_active: user.is_active,
                role,
                created_at: user.created_at,
                mfa_enrolled_at: user.mfa_enrolled_at,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

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

    let target = user.ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // The role must be one of the assignable tier roles.
    if !crate::models::role::is_tier(&payload.role) {
        return Err(ApiError::BadRequest(format!(
            "unknown role: {}",
            payload.role
        )));
    }
    let old_role = state
        .db
        .user_primary_role(user_id)
        .map_err(ApiError::from)?;

    // Set the tier role in user_roles (the users.role column is retired).
    state
        .db
        .set_user_primary_role(user_id, &payload.role)
        .map_err(|e| ApiError::from_db("Failed to update user role", e))?;

    let roster_user = RosterUser {
        id: target.id,
        username: target.username.clone(),
        email: target.email.clone(),
        full_name: target.full_name.clone(),
        is_active: target.is_active,
        role: payload.role.clone(),
        created_at: target.created_at,
        mfa_enrolled_at: target.mfa_enrolled_at,
    };

    // Log the role change
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserRoleChange,
            Some(target.id),
            Some(_admin_user.0.id),
            serde_json::json!({
                "old_role": old_role,
                "new_role": payload.role,
                "username": target.username,
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
        profile: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
        meta: None,
    };

    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(|e| ApiError::from_db("Failed to activate user", e))?;

    let role = state
        .db
        .user_primary_role(updated_user.id)
        .map_err(ApiError::from)?;
    let roster_user = RosterUser {
        id: updated_user.id,
        username: updated_user.username.clone(),
        email: updated_user.email.clone(),
        full_name: updated_user.full_name,
        is_active: updated_user.is_active,
        role,
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
        profile: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
        meta: None,
    };

    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(|e| ApiError::from_db("Failed to deactivate user", e))?;

    let role = state
        .db
        .user_primary_role(updated_user.id)
        .map_err(ApiError::from)?;
    let roster_user = RosterUser {
        id: updated_user.id,
        username: updated_user.username.clone(),
        email: updated_user.email.clone(),
        full_name: updated_user.full_name,
        is_active: updated_user.is_active,
        role,
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
///
/// Deliberately three short critical sections rather than one long one. The
/// first version of this held `pages_service.write().await` -- a tokio lock, on
/// the runtime that serves every request -- across a `git pull`, which queued
/// every reader of `/api/pages/*` behind an administrator pressing a button
/// (#94). The fetch needs the configuration and a path, not the store.
async fn refresh_wiki_pages(
    _admin_user: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    refresh_pages(state, PageType::Wiki).await
}

/// Refresh site pages from repository (admin only)
async fn refresh_site_pages(
    _admin_user: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    refresh_pages(state, PageType::Site).await
}

/// The body of both refresh handlers.
///
/// One function rather than two near-identical ones: the lock discipline here
/// is the whole point of #94, and two copies of it is two places for the next
/// edit to reintroduce the bug in only one of them.
async fn refresh_pages(
    state: AppState,
    page_type: PageType,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let kind = match page_type {
        PageType::Wiki => "wiki",
        PageType::Site => "site",
    };

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
    let configured = {
        let config = state.config_manager.get_config();
        match page_type {
            PageType::Wiki => config.pages.wiki_repo.is_some(),
            PageType::Site => config.pages.site_repo.is_some(),
        }
    };
    if !configured {
        return Err(ApiError::Conflict(format!(
            "No {kind} repository is configured; set one before refreshing"
        )));
    }

    // First critical section: copy what the fetch needs, then let go. Readers
    // are held up for three clones, not for a network round trip.
    let inputs = {
        let pages_service = state.pages_service.read().await;
        pages_service.refresh_inputs(page_type)
    };
    let Some((config, repo_path, gate)) = inputs else {
        return Err(ApiError::Conflict(format!(
            "No {kind} repository is configured; set one before refreshing"
        )));
    };

    // No lock held here at all. `prepare` takes the sync gate itself, which
    // serialises this against the background updater and against another
    // administrator without making anybody's page request wait.
    let prepared = PagesService::prepare(&config, &repo_path, page_type, &gate)
        .await
        .map_err(|e| {
            tracing::error!("Failed to refresh {} pages: {}", kind, e);
            ApiError::InternalServerError(format!("Failed to refresh {kind} pages: {e}"))
        })?;

    // Second critical section: swap the store. A map move and a vector move.
    let count = {
        let mut pages_service = state.pages_service.write().await;
        pages_service.publish(prepared)
    };

    // Built rather than `json!`-ed: the count's key varies with the page type,
    // and `json!` wants a literal there.
    let mut data = serde_json::Map::new();
    data.insert(format!("{kind}_pages_count"), serde_json::json!(count));
    data.insert(
        "updated_at".to_string(),
        serde_json::json!(chrono::Utc::now().to_rfc3339()),
    );

    Ok(Json(ApiResponse::success_with_message(
        serde_json::Value::Object(data),
        format!(
            "{} pages refreshed successfully. {} pages loaded.",
            match page_type {
                PageType::Wiki => "Wiki",
                PageType::Site => "Site",
            },
            count
        ),
    )))
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
