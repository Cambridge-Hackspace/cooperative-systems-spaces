use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};

use crate::{
    api::{
        errors::ApiError,
        responses::{ApiResponse, LoginRequest, LoginResponse, RegisterRequest, UserResponse},
    },
    auth::{AuthService, AuthUser, PasswordHashUtil},
    models::NewUser,
    AppState,
};

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/me", get(me))
        .route("/logout", post(logout))
}

// User registration endpoint
async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Basic validation
    if payload.username.is_empty() || payload.email.is_empty() || payload.password.is_empty() {
        return Err(ApiError::BadRequest("Username, email, and password are required".to_string()));
    }

    if payload.password.len() < 8 {
        return Err(ApiError::BadRequest("Password must be at least 8 characters long".to_string()));
    }

    if !payload.email.contains('@') {
        return Err(ApiError::BadRequest("Invalid email format".to_string()));
    }

    // Check if username already exists
    if let Ok(Some(_)) = state.db.find_user_by_username(&payload.username) {
        return Err(ApiError::Conflict("Username already exists".to_string()));
    }

    // Check if email already exists
    if let Ok(Some(_)) = state.db.find_user_by_email(&payload.email) {
        return Err(ApiError::Conflict("Email already exists".to_string()));
    }

    // Hash password
    let password_hash = PasswordHashUtil::hash(&payload.password)
        .map_err(|_| ApiError::InternalServerError("Failed to hash password".to_string()))?;

    // Check if this should be the first admin user
    let config = state.config_manager.get_config();
    let should_be_admin = config.should_grant_admin_role(&payload.email);

    // Create new user with appropriate role
    let new_user = if should_be_admin {
        use crate::models::UserRole;
        NewUser::with_role(
            payload.username,
            payload.email,
            password_hash,
            payload.full_name,
            UserRole::Admin,
        )
    } else {
        NewUser::new(
            payload.username,
            payload.email,
            password_hash,
            payload.full_name,
        )
    };

    let created_user = state.db.create_user(&new_user)
        .map_err(ApiError::from)?;

    // Log the registration event
    if let Err(e) = state.audit_logger.log_user_registration(
        created_user.id,
        &created_user.username,
        &created_user.email,
        "Newbie", // Default role for new users
        None, // No IP tracking for now
        None, // No User-Agent tracking for now
    ).await {
        tracing::warn!("Failed to log user registration: {}", e);
    }

    let message = if should_be_admin {
        "Admin user registered successfully".to_string()
    } else {
        "User registered successfully".to_string()
    };

    Ok(Json(ApiResponse::success_with_message(
        UserResponse::from(created_user),
        message,
    )))
}

// User login endpoint
async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<ApiResponse<LoginResponse>>, ApiError> {
    if payload.username_or_email.is_empty() || payload.password.is_empty() {
        return Err(ApiError::BadRequest("Username/email and password are required".to_string()));
    }

    let config = state.config_manager.get_config();
    let auth_service = AuthService::new(
        &state.db,
        &config.auth.jwt_secret,
    );

    let user = auth_service
        .authenticate_user(&payload.username_or_email, &payload.password)
        .map_err(ApiError::from)?;

    let token = auth_service
        .create_token(&user)
        .map_err(ApiError::from)?;

    let response = LoginResponse {
        token,
        user: UserResponse::from(user.clone()),
        expires_in: 24 * 60 * 60, // 24 hours in seconds
    };

    // Log the successful login
    if let Err(e) = state.audit_logger.log_event(
        crate::models::AuditEventType::UserLogin,
        Some(user.id),
        None,
        serde_json::json!({"username": user.username, "action": "User logged in successfully"}),
        None, // No IP tracking for now
        None, // No User-Agent tracking for now
    ).await {
        tracing::warn!("Failed to log user login: {}", e);
    }

    Ok(Json(ApiResponse::success_with_message(
        response,
        "Login successful".to_string(),
    )))
}

// Get current user info (protected endpoint)
async fn me(auth_user: AuthUser) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    Ok(Json(ApiResponse::success(UserResponse::from(auth_user.0))))
}

// Logout endpoint (for completeness, JWT is stateless)
async fn logout() -> Result<Json<ApiResponse<()>>, ApiError> {
    // Since JWT tokens are stateless, we can't invalidate them server-side
    // In a real application, you might want to implement a token blacklist
    // or use refresh tokens with short-lived access tokens
    Ok(Json(ApiResponse::success_with_message(
        (),
        "Logout successful. Please remove the token from client storage.".to_string(),
    )))
}
