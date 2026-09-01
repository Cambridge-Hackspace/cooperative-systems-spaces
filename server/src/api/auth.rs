use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};

use crate::{
    api::{
        errors::ApiError,
        responses::{
            ApiResponse, LoginRequest, LoginResponse, PasswordResetConsumeRequest,
            PasswordResetRequest, RegisterRequest, UserResponse,
        },
    },
    auth::{AuthService, AuthUser, PasswordHashUtil},
    models::{AuditEventType, NewUser, UpdateUser},
    tokens::{generate_token, hash_token, RESET_TOKEN_TTL_MINUTES},
    AppState,
};

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/me", get(me))
        .route("/logout", post(logout))
        .route("/password-reset/request", post(password_reset_request))
        .route("/password-reset/consume", post(password_reset_consume))
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

// ---------------------------------------------------------------------------
// Password reset
// ---------------------------------------------------------------------------

/// Attempts allowed before a reset address is locked out, and for how long.
///
/// Constants rather than config: no operator has asked to tune them, and every
/// config field costs two tracked files, a check, and a `PublicConfig`
/// question.
const RESET_THROTTLE_ATTEMPTS: u32 = 3;
const RESET_THROTTLE_SECONDS: u32 = 900;

/// The one thing `/password-reset/request` ever says.
///
/// A single constant, referenced twice, so the two branches cannot drift into
/// saying subtly different things -- which is how an enumeration oracle gets
/// built out of a helpful error message.
const RESET_REQUESTED_MESSAGE: &str =
    "If an account exists for that address, a password reset link has been sent.";

/// Is account recovery available at all on this deployment?
///
/// Both halves matter. `password_reset_enabled` is the operator's intent;
/// `email.enabled` is whether the intent can be carried out. The flag defaults
/// to true and the mailer defaults to off, so the shipped pair is "wanted but
/// impossible" -- and offering a form that cannot work is the same broken
/// promise this whole feature exists to stop making.
fn reset_available(config: &crate::config::AppConfig) -> bool {
    config.auth.password_reset_enabled && config.email.enabled
}

/// `POST /api/auth/password-reset/request`
///
/// Answers identically whether or not the address belongs to an account. That
/// is the entire security design of this endpoint, and it constrains
/// everything else in it:
///
///   - the status, body and message are the same on both branches;
///   - a failed send does **not** become a 5xx, because a 500 that is only
///     reachable when the account exists is a perfect enumeration oracle built
///     out of good intentions. The operator learns about it from the
///     `email_send_failed` audit row instead, which is a better channel anyway;
///   - the throttle records an attempt whether or not the account was found,
///     because a 429 that only ever appears for real addresses is the same
///     oracle wearing a different hat.
///
/// What it does **not** close is timing: the found branch hashes, writes twice
/// and opens an SMTP connection. Closing that needs either a fixed-delay
/// response or a background send, and a background send would reintroduce
/// exactly the fire-and-forget pattern the mailer was written to avoid. It is
/// recorded as a known limit rather than papered over.
async fn password_reset_request(
    State(state): State<AppState>,
    Json(payload): Json<PasswordResetRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let config = state.config_manager.get_config();
    if !reset_available(&config) {
        return Err(ApiError::Forbidden(
            "Password reset is not available on this instance".to_string(),
        ));
    }

    let address = payload.email.trim().to_lowercase();
    let throttle_identifier = format!("password-reset:{address}");

    if let Err(remaining_seconds) = state.throttle_service.check_attempt(
        &throttle_identifier,
        RESET_THROTTLE_ATTEMPTS,
        RESET_THROTTLE_SECONDS,
    ) {
        return Err(ApiError::TooManyRequests(format!(
            "Too many password reset requests. Try again in {remaining_seconds} seconds"
        )));
    }
    // Unconditionally, before the lookup: see the doc comment.
    state.throttle_service.record_failed_attempt(
        &throttle_identifier,
        RESET_THROTTLE_ATTEMPTS,
        RESET_THROTTLE_SECONDS,
    );

    let found = state
        .db
        .find_user_by_email(&address)
        .map_err(|e| ApiError::from_db("Failed to look up a reset address", e))?;

    if let Some(user) = found {
        let (plaintext, digest) = generate_token();
        let expires_at = chrono::Utc::now() + chrono::Duration::minutes(RESET_TOKEN_TTL_MINUTES);

        state
            .db
            .create_password_reset_token(user.id, digest, expires_at)
            .map_err(|e| ApiError::from_db("Failed to store a reset token", e))?;

        let link = format!(
            "{}/reset-password?token={}",
            config.site.site_url.trim_end_matches('/'),
            plaintext
        );
        let body = format!(
            "Somebody asked to reset the password for your {} account.\n\n             To choose a new one, open this link:\n\n{}\n\n             The link works once and expires in {} minutes. If you did not ask \n             for this, you can ignore this message -- your password has not \n             changed.\n",
            config.site.site_name, link, RESET_TOKEN_TTL_MINUTES
        );

        // The send result is used, never discarded -- but it is used to inform
        // the operator, not the requester.
        match state
            .mail_service
            .send(&user.email, "Reset your password", &body)
            .await
        {
            Ok(()) => {
                let _ = state
                    .audit_logger
                    .log_event(
                        AuditEventType::PasswordResetRequested,
                        Some(user.id),
                        Some(user.id),
                        serde_json::json!({ "found": true }),
                        None,
                        None,
                    )
                    .await;
            }
            Err(e) => {
                tracing::error!("password reset mail failed: {e}");
                let _ = state
                    .audit_logger
                    .log_event(
                        AuditEventType::EmailSendFailed,
                        Some(user.id),
                        Some(user.id),
                        serde_json::json!({
                            "purpose": "password_reset",
                            "error": e.to_string(),
                        }),
                        None,
                        None,
                    )
                    .await;
            }
        }
    } else {
        // Recorded with no subject, because there is none. This row is the only
        // place the answer the requester is denied gets written down.
        let _ = state
            .audit_logger
            .log_event(
                AuditEventType::PasswordResetRequested,
                None,
                None,
                serde_json::json!({ "found": false }),
                None,
                None,
            )
            .await;
    }

    Ok(Json(ApiResponse::success_with_message(
        (),
        RESET_REQUESTED_MESSAGE.to_string(),
    )))
}

/// `POST /api/auth/password-reset/consume`
///
/// Deliberately returns **no** token. The user is sent to the login form.
///
/// That is not a usability oversight, it is what preserves MFA. If this issued
/// a session, anyone controlling a mailbox would obtain full account access
/// without ever passing a second factor -- a password reset would be a silent
/// MFA bypass. Sending the user through `POST /api/auth/login` means
/// `mfa_enrolled_at` is honored by construction rather than by somebody
/// remembering to check it here. For the same reason this must never clear
/// `mfa_enrolled_at` or delete an MFA row.
///
/// Every token failure is **400, never 401**. Two reasons, and the first is
/// concrete: `frontend/src/utils/api.ts` calls `authStore.logout()` on any 401
/// from any endpoint, so a signed-in user who pastes a stale link would be
/// signed out, and would experience it as a mysterious session expiry rather
/// than as a stale link. The second is that the token is a parameter of the
/// request, not a credential authenticating the caller.
async fn password_reset_consume(
    State(state): State<AppState>,
    Json(payload): Json<PasswordResetConsumeRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let config = state.config_manager.get_config();
    if !reset_available(&config) {
        return Err(ApiError::Forbidden(
            "Password reset is not available on this instance".to_string(),
        ));
    }

    // Validated before the lookup: it costs nothing, needs no database, and
    // putting it first is what makes it assertable in the offline contract
    // tier, where every query reaches a dead pool.
    if payload.new_password.len() < config.auth.password_min_length {
        return Err(ApiError::ValidationError(format!(
            "Password must be at least {} characters long",
            config.auth.password_min_length
        )));
    }

    let claimed = state
        .db
        .claim_password_reset_token(&hash_token(payload.token.trim()))
        .map_err(|e| ApiError::from_db("Failed to claim a reset token", e))?;

    let Some(user_id) = claimed else {
        let _ = state
            .audit_logger
            .log_event(
                AuditEventType::PasswordResetFailed,
                None,
                None,
                serde_json::json!({ "reason": "unknown, expired, or already used" }),
                None,
                None,
            )
            .await;
        // One message for all three causes. Telling the requester which of them
        // applied would say whether the token ever existed.
        return Err(ApiError::BadRequest(
            "This password reset link is invalid or has expired. Request a new one.".to_string(),
        ));
    };

    let password_hash = PasswordHashUtil::hash(&payload.new_password)
        .map_err(|_| ApiError::InternalServerError("Failed to hash password".to_string()))?;

    state
        .db
        .update_user(
            user_id,
            &UpdateUser {
                username: None,
                email: None,
                password_hash: Some(password_hash),
                full_name: None,
                is_active: None,
                role: None,
                profile: None,
                meta: None,
                updated_at: Some(chrono::Utc::now().naive_utc()),
            },
        )
        .map_err(|e| ApiError::from_db("Failed to set a password from a reset token", e))?;

    let _ = state
        .audit_logger
        .log_event(
            AuditEventType::PasswordResetCompleted,
            Some(user_id),
            Some(user_id),
            serde_json::json!({
                "note": "password changed without the previous one being presented",
            }),
            None,
            None,
        )
        .await;

    Ok(Json(ApiResponse::success_with_message(
        (),
        "Your password has been changed. You can now sign in with it.".to_string(),
    )))
}
