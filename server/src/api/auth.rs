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
        .nest("/mfa", crate::api::mfa::mfa_routes())
}

// User registration endpoint
async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<ApiResponse<UserResponse>>, ApiError> {
    // Check if registration is allowed
    let config = state.config_manager.get_config();
    if !config.auth.allow_registration {
        return Err(ApiError::Forbidden(
            "User registration is disabled".to_string(),
        ));
    }

    // Get client identifier for throttling (using email as identifier)
    let throttle_identifier = payload.email.to_lowercase();

    // Check registration challenge if enabled
    if config.registration_challenge.enabled {
        let challenge_phrase = payload.challenge_phrase.as_deref().unwrap_or("");

        // Check throttling before processing the challenge
        if config.registration_challenge.throttle_enabled {
            if let Err(remaining_seconds) = state.throttle_service.check_attempt(
                &throttle_identifier,
                config.registration_challenge.throttle_attempts,
                config.registration_challenge.throttle_seconds,
            ) {
                return Err(ApiError::TooManyRequests(format!(
                    "Too many failed registration attempts. Try again in {} seconds",
                    remaining_seconds
                )));
            }
        }

        // Validate the challenge phrase
        if challenge_phrase != config.registration_challenge.phrase {
            // Record failed attempt if throttling is enabled
            if config.registration_challenge.throttle_enabled {
                state.throttle_service.record_failed_attempt(
                    &throttle_identifier,
                    config.registration_challenge.throttle_attempts,
                    config.registration_challenge.throttle_seconds,
                );
            }
            return Err(ApiError::BadRequest(
                "Invalid registration phrase".to_string(),
            ));
        }
    }

    // Check terms of service acceptance if required
    if config.registration_challenge.terms_of_service_checkbox {
        if !payload.terms_of_service_accepted.unwrap_or(false) {
            return Err(ApiError::BadRequest(
                "You must accept the terms of service to register".to_string(),
            ));
        }
    }

    // Check reCAPTCHA if enabled
    if config.registration_challenge.recaptcha_enabled {
        let recaptcha_token = payload.recaptcha_token.as_deref().unwrap_or("");

        if recaptcha_token.is_empty() {
            return Err(ApiError::BadRequest(
                "reCAPTCHA verification is required".to_string(),
            ));
        }

        // Verify reCAPTCHA token
        match state
            .recaptcha_service
            .verify_token(recaptcha_token, None)
            .await
        {
            Ok(true) => {
                tracing::debug!("reCAPTCHA verification successful for registration");
            }
            Ok(false) => {
                tracing::warn!("reCAPTCHA verification failed for registration");
                // Record failed attempt if throttling is enabled
                if config.registration_challenge.throttle_enabled {
                    state.throttle_service.record_failed_attempt(
                        &throttle_identifier,
                        config.registration_challenge.throttle_attempts,
                        config.registration_challenge.throttle_seconds,
                    );
                }
                return Err(ApiError::BadRequest(
                    "reCAPTCHA verification failed. Please try again.".to_string(),
                ));
            }
            Err(e) => {
                tracing::error!("reCAPTCHA verification error: {}", e);
                return Err(ApiError::InternalServerError(
                    "reCAPTCHA verification service unavailable".to_string(),
                ));
            }
        }
    }

    // Basic validation
    if payload.username.is_empty() || payload.email.is_empty() || payload.password.is_empty() {
        return Err(ApiError::BadRequest(
            "Username, email, and password are required".to_string(),
        ));
    }

    if payload.password.len() < config.auth.password_min_length {
        return Err(ApiError::BadRequest(format!(
            "Password must be at least {} characters long",
            config.auth.password_min_length
        )));
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

    let created_user = state.db.create_user(&new_user).map_err(ApiError::from)?;

    // Clear throttle on successful registration
    if config.registration_challenge.enabled && config.registration_challenge.throttle_enabled {
        state
            .throttle_service
            .record_successful_attempt(&throttle_identifier);
    }

    // Log the registration event
    if let Err(e) = state
        .audit_logger
        .log_user_registration(
            created_user.id,
            &created_user.username,
            &created_user.email,
            "Newbie", // Default role for new users
            None,     // No IP tracking for now
            None,     // No User-Agent tracking for now
        )
        .await
    {
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

// User login endpoint. Returns either a JWT directly, or — if the user has
// MFA enrolled — an MFA challenge envelope the client trades back to
// `/api/auth/mfa/verify` for a JWT.
async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    if payload.username_or_email.is_empty() || payload.password.is_empty() {
        return Err(ApiError::BadRequest(
            "Username/email and password are required".to_string(),
        ));
    }

    let config = state.config_manager.get_config();
    let auth_service = AuthService::new(&state.db, &config.auth.jwt_secret);

    let user = auth_service
        .authenticate_user(&payload.username_or_email, &payload.password)
        .map_err(ApiError::from)?;

    // If the user has confirmed any MFA method, issue a challenge instead of
    // a token. They complete login via /api/auth/mfa/verify.
    if user.mfa_enrolled_at.is_some() && config.auth.mfa.enabled {
        let challenge = crate::api::mfa::build_login_challenge(&state, &user)?;
        return Ok(Json(ApiResponse::success_with_message(
            challenge,
            "MFA challenge required".to_string(),
        )));
    }

    let token = auth_service.create_token(&user).map_err(ApiError::from)?;

    // Flag users whose role requires enrollment under the current policy so
    // the frontend can route them to the enrollment page on first sight.
    let must_enroll = config.auth.mfa.is_required_for(&user.role) && user.mfa_enrolled_at.is_none();

    let response = LoginResponse {
        token,
        user: UserResponse::from(user.clone()),
        expires_in: (config.auth.jwt_expiration_hours as i64) * 60 * 60,
        must_enroll_mfa: if must_enroll { Some(true) } else { None },
    };

    // Log the successful login
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::UserLogin,
            Some(user.id),
            None,
            serde_json::json!({"username": user.username, "action": "User logged in successfully"}),
            None,
            None,
        )
        .await
    {
        tracing::warn!("Failed to log user login: {}", e);
    }

    let value = serde_json::to_value(&response).map_err(|e| {
        ApiError::InternalServerError(format!("Failed to serialize login response: {e}"))
    })?;
    Ok(Json(ApiResponse::success_with_message(
        value,
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
