use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{delete, get, put},
    Router,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    api::{
        errors::ApiError,
        responses::{ApiResponse, PaginatedResponse, PaginationParams, UpdateUserRequest, UserResponse},
    },
    auth::{AdminUser, AuthUser, StaffUser, PasswordHashUtil},
    models::{UpdateUser, UserRole},
    AppState,
};

pub fn user_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_users))
        .route("/:id", get(get_user_by_id))
        .route("/:id", put(update_user))
        .route("/:id", delete(delete_user))
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
        return Err(ApiError::BadRequest("Invalid pagination parameters".to_string()));
    }

    let offset = ((page - 1) * per_page) as i64;
    let limit = per_page as i64;

    // Get total count
    let total_count = state.db.count_users()
        .map_err(ApiError::from)? as u32;

    // Get users (we'll implement this method in database.rs)
    let users = state.db.list_users_paginated(limit, offset)
        .map_err(ApiError::from)?;

    let user_responses: Vec<UserResponse> = users
        .into_iter()
        .map(UserResponse::from)
        .collect();

    let paginated_response = PaginatedResponse::new(
        user_responses,
        page,
        per_page,
        total_count,
    );

    Ok(Json(ApiResponse::success(paginated_response)))
}

// Get user by ID
async fn get_user_by_id(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Users can view their own profile, or staff/admin users can view any profile
    if auth_user.0.id != user_id && !auth_user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden("You can only view your own profile".to_string()));
    }

    let user = state.db.find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    Ok(Json(ApiResponse::success(UserResponse::from(user))))
}

// Update user
async fn update_user(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Users can only update their own profile, unless they're staff/admin
    if auth_user.0.id != user_id && !auth_user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden("You can only update your own profile".to_string()));
    }

    // Non-staff users cannot update is_active or role
    if !auth_user.0.role.can_access_staff() && (payload.is_active.is_some() || payload.role.is_some()) {
        return Err(ApiError::Forbidden("You cannot modify account status or role".to_string()));
    }

    // Only admins can set admin role
    if let Some(ref new_role) = payload.role {
        if *new_role == UserRole::Admin && !auth_user.0.role.can_access_admin() {
            return Err(ApiError::Forbidden("Only admins can assign admin role".to_string()));
        }
    }

    // Check if user exists
    let existing_user = state.db.find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Prepare update data
    let mut update_data = UpdateUser {
        username: payload.username,
        email: payload.email,
        full_name: payload.full_name,
        password_hash: None,
        is_active: payload.is_active,
        role: payload.role,
        updated_at: Some(Utc::now().naive_utc()),
    };

    // Hash new password if provided
    if let Some(new_password) = payload.password {
        if new_password.len() < 8 {
            return Err(ApiError::BadRequest("Password must be at least 8 characters long".to_string()));
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
    let updated_user = state.db.update_user(user_id, &update_data)
        .map_err(ApiError::from)?;

    Ok(Json(ApiResponse::success_with_message(
        UserResponse::from(updated_user),
        "User updated successfully".to_string(),
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
        return Err(ApiError::BadRequest("You cannot delete your own account".to_string()));
    }

    // Check if user exists
    state.db.find_user_by_id(user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    // Delete user
    state.db.delete_user(user_id)
        .map_err(ApiError::from)?;

    Ok(Json(ApiResponse::success_with_message(
        (),
        "User deleted successfully".to_string(),
    )))
}