use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{delete, get, patch, put},
    Router,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    api::{
        errors::ApiError,
        responses::{
            ApiResponse, PaginatedResponse, PaginationParams, UpdateUserRequest, UserResponse,
        },
    },
    auth::{AdminUser, AuthUser, PasswordHashUtil},
    models::{AuditEventType, NewAuditLog, UpdateUser},
    profile::AuditLogger,
    AppState,
};

pub fn user_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users))
        .route("/me/password", put(change_own_password))
        .route("/{id}", get(get_user_by_id))
        .route("/{id}", put(update_user))
        .route("/{id}", delete(delete_user))
        .route("/{id}/theme", patch(update_user_theme))
}

fn audit(state: &AppState, event: AuditEventType, actor: Uuid, data: serde_json::Value) {
    let log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: Some(actor),
        actor_id: Some(actor),
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write user audit log {}: {}", event.as_str(), e);
    }
}

// List all users (admin only)
async fn list_users(
    _admin_user: AdminUser, // Ensures only admin users can access
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<PaginatedResponse<UserResponse>>>, ApiError> {
    let page = params.page.unwrap_or(1);
    let per_page = params.per_page.unwrap_or(20);

    if page == 0 || per_page == 0 || per_page > 100 {
        return Err(ApiError::BadRequest(
            "Invalid pagination parameters".to_string(),
        ));
    }

    let offset = ((page - 1) * per_page) as i64;
    let limit = per_page as i64;

    // Get total count
    let total_count = state.db.count_active_users().map_err(ApiError::from)? as u32;

    // Get users (we'll implement this method in database.rs)
    let users = state
        .db
        .list_users_paginated(limit, offset)
        .map_err(ApiError::from)?;

    let user_responses: Vec<UserResponse> = users
        .into_iter()
        .map(|u| {
            let role = state.db.user_primary_role(u.id).map_err(ApiError::from)?;
            Ok::<_, ApiError>(UserResponse::from_user(u, role))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let paginated_response = PaginatedResponse::new(user_responses, page, per_page, total_count);

    Ok(Json(ApiResponse::success(paginated_response)))
}

// Get user by ID
async fn get_user_by_id(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Users can view their own profile, or staff/admin users can view any profile
    if auth_user.0.id != user_id
        && !state
            .db
            .user_has_permission(auth_user.0.id, "users.manage")
            .map_err(ApiError::from)?
    {
        return Err(ApiError::Forbidden(
            "You can only view your own profile".to_string(),
        ));
    }

    let user = state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    let role = state
        .db
        .user_primary_role(user.id)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(UserResponse::from_user(
        user, role,
    ))))
}

// Update user
async fn update_user(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Users can only update their own profile, unless they're staff/admin
    if auth_user.0.id != user_id
        && !state
            .db
            .user_has_permission(auth_user.0.id, "users.manage")
            .map_err(ApiError::from)?
    {
        return Err(ApiError::Forbidden(
            "You can only update your own profile".to_string(),
        ));
    }

    // Non-staff users cannot update is_active or role
    if !state
        .db
        .user_has_permission(auth_user.0.id, "users.manage")
        .map_err(ApiError::from)?
        && (payload.is_active.is_some() || payload.role.is_some())
    {
        return Err(ApiError::Forbidden(
            "You cannot modify account status or role".to_string(),
        ));
    }

    // A requested role must be one of the assignable tier roles.
    if let Some(ref new_role) = payload.role {
        if !crate::models::role::is_tier(new_role) {
            return Err(ApiError::BadRequest(format!("unknown role: {new_role}")));
        }
        // Only admins can grant the admin role.
        if new_role == crate::models::role::ADMIN
            && !state
                .db
                .user_has_permission(auth_user.0.id, "admin.access")
                .map_err(ApiError::from)?
        {
            return Err(ApiError::Forbidden(
                "Only admins can assign admin role".to_string(),
            ));
        }
    }

    // Check if user exists
    let existing_user = state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // #120/#1: `users.manage` by itself let a manager act on a user at or above
    // their own level -- set an admin's password or email, deactivate or demote
    // them, then log in as that admin. A manager may act only on users strictly
    // below their own effective level. Editing one's own record is permitted by
    // the ownership check above and is exempt here.
    if auth_user.0.id != user_id {
        let caller_level = state
            .db
            .user_effective_level(auth_user.0.id)
            .map_err(ApiError::from)?;
        let target_level = state
            .db
            .user_effective_level(user_id)
            .map_err(ApiError::from)?;
        if target_level >= caller_level {
            return Err(ApiError::Forbidden(
                "You cannot modify a user at or above your own access level".to_string(),
            ));
        }
    }

    // #120/#2: a self-service change to a credential -- the account password or
    // the email address -- requires the current password, so a stolen token
    // cannot re-key or re-address the account. Checked once here for either
    // credential (the password and email blocks below no longer re-check). A
    // manager acting on another user is the admin path, gated by the level check
    // above, and supplies no current password.
    let email_changing = payload
        .email
        .as_deref()
        .is_some_and(|e| e != existing_user.email);
    if auth_user.0.id == user_id && (payload.password.is_some() || email_changing) {
        let current_ok = payload
            .current_password
            .as_deref()
            .map(|c| PasswordHashUtil::verify(c, &existing_user.password_hash).unwrap_or(false))
            .unwrap_or(false);
        if !current_ok {
            return Err(ApiError::BadRequest(
                "Current password is incorrect".to_string(),
            ));
        }
    }

    // The tier role (if requested) is applied to user_roles after the row
    // update, since it no longer lives on the users table.
    let requested_role = payload.role.clone();

    // Prepare update data
    let mut update_data = UpdateUser {
        username: payload.username,
        email: payload.email,
        full_name: payload.full_name,
        password_hash: None,
        is_active: payload.is_active,
        profile: None, // For now, profile updates will be handled separately
        updated_at: Some(Utc::now().naive_utc()),
        meta: None, // Meta is system-managed, not user-editable via this endpoint
    };

    // Hash new password if provided. The current-password re-auth for a
    // self-service change is enforced once, up front, for either credential.
    if let Some(new_password) = payload.password {
        // The minimum length is configured, not hardcoded (#120/#2): the 8 here
        // ignored auth.password_min_length, so a deployment that raised it still
        // accepted short passwords through this path.
        let min_len = state.config_manager.get_config().auth.password_min_length;
        if new_password.len() < min_len {
            return Err(ApiError::BadRequest(format!(
                "Password must be at least {min_len} characters long"
            )));
        }

        let password_hash = PasswordHashUtil::hash(&new_password)
            .map_err(|_| ApiError::InternalServerError("Failed to hash password".to_string()))?;

        update_data.password_hash = Some(password_hash);
    }

    // Validate email uniqueness if email is being changed
    if let Some(ref email) = update_data.email {
        if email != &existing_user.email {
            if !email.contains('@') {
                return Err(ApiError::BadRequest("Invalid email format".to_string()));
            }

            if let Ok(Some(_)) = state.db.find_user_by_email(email) {
                return Err(ApiError::Conflict("Email already exists".to_string()));
            }
        }
    }

    // Validate username uniqueness if username is being changed
    if let Some(ref username) = update_data.username {
        if username != &existing_user.username {
            if username.is_empty() {
                return Err(ApiError::BadRequest("Username cannot be empty".to_string()));
            }

            if let Ok(Some(_)) = state.db.find_user_by_username(username) {
                return Err(ApiError::Conflict("Username already exists".to_string()));
            }
        }
    }

    // Update user
    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(ApiError::from)?;

    // #120/#2: a changed address is no longer proven. Clear its verified flag so
    // require_email_verification re-gates, and send a fresh confirmation to the
    // NEW address. Applies to an admin-set email too: whoever set it, the new
    // address has not demonstrated ownership.
    if email_changing {
        state
            .db
            .clear_email_verified_at(user_id)
            .map_err(ApiError::from)?;
        crate::api::auth::issue_verification_mail(&state, &updated_user).await;
    }

    // Apply a requested tier-role change through user_roles (validated above).
    if let Some(new_role) = requested_role {
        state
            .db
            .set_user_primary_role(user_id, &new_role)
            .map_err(ApiError::from)?;
    }

    // Broadcast toolguard state if active status changed (affects tool access)
    if update_data.is_active.is_some() {
        crate::api::toolguard::broadcast_toolguard_state(&state).await;
    }

    // Record an email change so the mailing-list sync can follow it. This path
    // otherwise emits no audit event, so without this a changed address would
    // stay on the Groups.io list under the old value until the next full
    // reconciliation -- the sync consumes UserEmailChange to move it at once.
    if let Some(ref new_email) = update_data.email {
        if new_email != &existing_user.email {
            if let Err(e) = state
                .audit_logger
                .log_event(
                    AuditEventType::UserEmailChange,
                    Some(user_id),
                    Some(auth_user.0.id),
                    serde_json::json!({
                        "old_email": existing_user.email,
                        "new_email": new_email,
                    }),
                    None,
                    None,
                )
                .await
            {
                tracing::warn!("Failed to log user email change: {}", e);
            }
        }
    }

    let role = state
        .db
        .user_primary_role(updated_user.id)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success_with_message(
        UserResponse::from_user(updated_user, role),
        "User updated successfully".to_string(),
    )))
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

// Self-service password change. Requires proof of the current password so a
// hijacked session (stolen token, unattended logged-in browser) can't be used
// to silently lock the real owner out -- unlike update_user's password field,
// which is only reachable by staff/admin acting on someone else's account and
// intentionally skips this check (that's the admin-reset path).
async fn change_own_password(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    if !PasswordHashUtil::verify(&payload.current_password, &auth_user.0.password_hash)
        .unwrap_or(false)
    {
        return Err(ApiError::BadRequest(
            "Current password is incorrect".to_string(),
        ));
    }

    let config = state.config_manager.get_config();
    if payload.new_password.len() < config.auth.password_min_length {
        return Err(ApiError::BadRequest(format!(
            "Password must be at least {} characters long",
            config.auth.password_min_length
        )));
    }

    let password_hash = PasswordHashUtil::hash(&payload.new_password)
        .map_err(|_| ApiError::InternalServerError("Failed to hash password".to_string()))?;

    let update_data = UpdateUser {
        username: None,
        email: None,
        password_hash: Some(password_hash),
        full_name: None,
        is_active: None,
        profile: None,
        meta: None,
        updated_at: Some(Utc::now().naive_utc()),
    };

    state
        .db
        .update_user(auth_user.0.id, &update_data)
        .map_err(ApiError::from)?;

    audit(
        &state,
        AuditEventType::UserPasswordChange,
        auth_user.0.id,
        serde_json::json!({}),
    );

    Ok(Json(ApiResponse::success_with_message(
        (),
        "Password changed successfully".to_string(),
    )))
}

// Delete user (admin only)
async fn delete_user(
    admin_user: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    // Admin cannot delete themselves
    if admin_user.0.id == user_id {
        return Err(ApiError::BadRequest(
            "You cannot delete your own account".to_string(),
        ));
    }

    // Check if user exists
    let victim = state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;
    let victim_role = state
        .db
        .user_primary_role(victim.id)
        .map_err(ApiError::from)?;

    // Audited BEFORE the delete, and with the subject in `event_data` rather
    // than in `user_id`. Two reasons, and both were found by running this:
    //
    //   * `audit_logs.user_id` is `REFERENCES users(id) ON DELETE SET NULL`, so
    //     a row inserted *after* the delete violates the constraint outright --
    //     and `create_audit_log` is called as `let _ = ..`, so the failure goes
    //     to a log line nobody reads and the record simply does not exist;
    //   * even a row inserted before the delete has its `user_id` set to NULL by
    //     the cascade a moment later, so the only durable record of *who* was
    //     deleted is the JSON.
    //
    // Until now this handler wrote nothing at all. The most destructive
    // administrative action in the system left no trace, which is the one place
    // an audit trail is not optional.
    let audit_logger = AuditLogger::new(state.db.clone());
    if let Err(e) = audit_logger
        .log_event(
            AuditEventType::UserDeletion,
            None,
            Some(admin_user.0.id),
            serde_json::json!({
                "deleted_user_id": victim.id,
                "deleted_username": victim.username,
                "deleted_email": victim.email,
                "deleted_role": victim_role,
                "action": "User deleted by admin",
            }),
            None,
            None,
        )
        .await
    {
        // Refused, not swallowed. If the deletion cannot be recorded, it does
        // not happen: an unrecorded deletion of a member account is worse than
        // a deletion that failed and can be retried.
        tracing::error!("Refusing to delete user {user_id}: audit write failed: {e}");
        return Err(ApiError::InternalServerError(
            "Could not record this deletion; the user was not deleted".to_string(),
        ));
    }

    // Delete user
    state.db.delete_user(user_id).map_err(ApiError::from)?;

    Ok(Json(ApiResponse::success_with_message(
        (),
        "User deleted successfully".to_string(),
    )))
}

// Request body for updating theme
#[derive(Debug, Deserialize)]
pub struct UpdateThemeRequest {
    pub theme: String,
}

// Update user theme preference (authenticated user can update their own)
async fn update_user_theme(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateThemeRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Users can only update their own theme (unless admin)
    if auth_user.0.id != user_id
        && !state
            .db
            .user_has_permission(auth_user.0.id, "admin.access")
            .map_err(ApiError::from)?
    {
        return Err(ApiError::Forbidden(
            "You can only update your own theme".to_string(),
        ));
    }

    // Validate theme value (must match themes in tailwind.config.js, plus
    // "system", which the frontend resolves to css-light/css-dark from the
    // browser's prefers-color-scheme rather than naming a daisyUI theme).
    //
    // frontend/tests/structure/themes.spec.ts asserts this list against
    // tailwind.config.js and ThemePicker.vue, so the three cannot drift.
    let valid_themes = [
        "system",
        "css-light",
        "css-dark",
        "afterdark",
        "her",
        "forest",
        "sky",
        "clays",
        "stones",
        "lofi",
        "black",
        "light",
        "dark",
        "cupcake",
        "corporate",
    ];
    if !valid_themes.contains(&payload.theme.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "Invalid theme: {}. Must be one of: {}",
            payload.theme,
            valid_themes.join(", ")
        )));
    }

    // Get existing user
    let existing_user = state
        .db
        .find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Update meta with new theme
    let mut meta = existing_user.meta.clone();
    meta["theme"] = serde_json::json!(payload.theme);

    let update_data = UpdateUser {
        username: None,
        email: None,
        password_hash: None,
        full_name: None,
        is_active: None,
        profile: None,
        meta: Some(meta),
        updated_at: Some(Utc::now().naive_utc()),
    };

    let updated_user = state
        .db
        .update_user(user_id, &update_data)
        .map_err(ApiError::from)?;

    let role = state
        .db
        .user_primary_role(updated_user.id)
        .map_err(ApiError::from)?;
    Ok(Json(ApiResponse::success(UserResponse::from_user(
        updated_user,
        role,
    ))))
}
