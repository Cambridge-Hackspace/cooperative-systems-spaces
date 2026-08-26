//! MFA enrollment and login-verification endpoints.
//!
//! Enrollment endpoints (`/api/auth/mfa/*` except `/verify`) require a normal
//! logged-in session. `/verify` is unauthenticated by design: the holder of a
//! valid login `challenge_token` is the one being challenged.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::auth::{AuthService, AuthUser};
use crate::mfa::{
    self, generate_recovery_codes, generate_totp_secret_base32, hash_recovery_code, methods,
    verify_recovery_code, LoginChallenge, WebauthnRegistration,
};
use crate::models::{AuditEventType, NewAuditLog, NewUserMfaWebauthn};
use crate::AppState;

use super::errors::ApiError;
use super::responses::{ApiResponse, LoginResponse, UserResponse};

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn mfa_routes() -> Router<AppState> {
    Router::new()
        // Public — completes the login flow using the challenge token.
        .route("/verify", post(verify_login))
        // Auth-required — enrollment surface.
        .route("/status", get(status))
        .route("/totp/setup", post(totp_setup))
        .route("/totp/confirm", post(totp_confirm))
        .route("/totp", delete(totp_disable))
        .route("/webauthn", get(webauthn_list))
        .route("/webauthn/{id}", delete(webauthn_remove))
        .route("/webauthn/register/begin", post(webauthn_register_begin))
        .route("/webauthn/register/finish", post(webauthn_register_finish))
        .route("/recovery-codes/regenerate", post(recovery_regenerate))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MfaStatusResponse {
    pub enabled: bool,
    pub totp_enrolled: bool,
    pub webauthn_count: usize,
    pub recovery_codes_remaining: i64,
    /// `true` when the configured enforcement requires this user to enroll
    /// but they haven't yet.
    pub must_enroll: bool,
}

#[derive(Debug, Serialize)]
pub struct MfaTotpSetupResponse {
    pub secret_base32: String,
    pub otpauth_uri: String,
}

#[derive(Debug, Deserialize)]
pub struct MfaTotpConfirmRequest {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct MfaRecoveryCodesResponse {
    pub recovery_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct WebauthnCredentialResponse {
    pub id: Uuid,
    pub label: String,
    pub created_at: chrono::DateTime<Utc>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct WebauthnRegisterBeginRequest {
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct WebauthnRegisterBeginResponse {
    pub challenge_token: String,
    pub options: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct WebauthnRegisterFinishRequest {
    pub challenge_token: String,
    pub response: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct WebauthnRegisterFinishResponse {
    pub credential_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct MfaVerifyRequest {
    pub challenge_token: String,
    pub method: String,
    /// Present for `totp` and `recovery`.
    pub code: Option<String>,
    /// Present for `webauthn`: the `PublicKeyCredential` returned by
    /// `navigator.credentials.get(...)`.
    pub response: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.mfa_service.config().enabled {
        return Err(ApiError::Forbidden(
            "MFA is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

fn audit(state: &AppState, event: AuditEventType, user_id: Uuid, data: serde_json::Value) {
    let log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: Some(user_id),
        actor_id: Some(user_id),
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write MFA audit log {}: {}", event.as_str(), e);
    }
}

fn issue_token(state: &AppState, user: &crate::models::User) -> Result<LoginResponse, ApiError> {
    let config = state.config_manager.get_config();
    let auth = AuthService::new(&state.db, &config.auth.jwt_secret);
    let token = auth.create_token(user).map_err(ApiError::from)?;
    Ok(LoginResponse {
        token,
        user: UserResponse::from(user.clone()),
        expires_in: (config.auth.jwt_expiration_hours as i64) * 60 * 60,
        must_enroll_mfa: None,
    })
}

// ---------------------------------------------------------------------------
// Status & enrollment endpoints
// ---------------------------------------------------------------------------

async fn status(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let cfg = state.mfa_service.config().clone();
    let totp = state.db.get_user_totp(user.0.id)?;
    let webauthn = state.db.list_user_webauthn(user.0.id)?;
    let recovery_remaining = state.db.count_unused_recovery_codes(user.0.id)?;
    let totp_enrolled = totp.as_ref().and_then(|t| t.confirmed_at).is_some();
    let webauthn_count = webauthn.len();
    let must_enroll = cfg.is_required_for(&user.0.role) && user.0.mfa_enrolled_at.is_none();
    Ok(Json(ApiResponse::success(MfaStatusResponse {
        enabled: cfg.enabled,
        totp_enrolled,
        webauthn_count,
        recovery_codes_remaining: recovery_remaining,
        must_enroll,
    })))
}

async fn totp_setup(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if !state.mfa_service.config().allow_totp {
        return Err(ApiError::Forbidden("TOTP disabled".to_string()));
    }
    let secret = generate_totp_secret_base32();
    state.db.replace_user_totp_unconfirmed(user.0.id, &secret)?;
    let totp = state
        .mfa_service
        .totp(&secret, &user.0.email)
        .map_err(ApiError::InternalServerError)?;
    Ok(Json(ApiResponse::success(MfaTotpSetupResponse {
        secret_base32: secret,
        otpauth_uri: totp.get_url(),
    })))
}

async fn totp_confirm(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<MfaTotpConfirmRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let stored = state
        .db
        .get_user_totp(user.0.id)?
        .ok_or_else(|| ApiError::BadRequest("No TOTP setup in progress".to_string()))?;
    if !state
        .mfa_service
        .verify_totp(&stored.secret_base32, &user.0.email, req.code.trim())
    {
        return Err(ApiError::BadRequest("Invalid code".to_string()));
    }
    state.db.confirm_user_totp(user.0.id)?;
    state.db.recompute_user_mfa_enrolled(user.0.id)?;

    // Generate recovery codes on first enrollment so the user has a fallback.
    let count = state.mfa_service.config().recovery_code_count;
    let codes = generate_recovery_codes(count);
    let hashes: Vec<String> = codes
        .iter()
        .map(|c| hash_recovery_code(c))
        .collect::<Result<_, _>>()
        .map_err(ApiError::InternalServerError)?;
    state.db.replace_user_recovery_codes(user.0.id, hashes)?;

    audit(
        &state,
        AuditEventType::MfaTotpEnrolled,
        user.0.id,
        serde_json::json!({}),
    );
    audit(
        &state,
        AuditEventType::MfaRecoveryCodesRegenerated,
        user.0.id,
        serde_json::json!({ "count": count }),
    );

    Ok(Json(ApiResponse::success(MfaRecoveryCodesResponse {
        recovery_codes: codes,
    })))
}

async fn totp_disable(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    state.db.delete_user_totp(user.0.id)?;
    state.db.recompute_user_mfa_enrolled(user.0.id)?;
    audit(
        &state,
        AuditEventType::MfaTotpDisabled,
        user.0.id,
        serde_json::json!({}),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("TOTP disabled".to_string()),
        error: None,
    }))
}

// --- WebAuthn enrollment --------------------------------------------------

async fn webauthn_list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let rows = state.db.list_user_webauthn(user.0.id)?;
    let resp: Vec<WebauthnCredentialResponse> = rows
        .into_iter()
        .map(|r| WebauthnCredentialResponse {
            id: r.id,
            label: r.label,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        })
        .collect();
    Ok(Json(ApiResponse::success(resp)))
}

async fn webauthn_register_begin(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<WebauthnRegisterBeginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let webauthn = state
        .mfa_service
        .webauthn()
        .ok_or_else(|| ApiError::Forbidden("WebAuthn disabled".to_string()))?;

    // Exclude already-registered credentials so the same key isn't added twice.
    let existing = state.db.list_user_webauthn(user.0.id)?;
    let exclude: Vec<CredentialID> = existing
        .iter()
        .map(|r| r.credential_id.clone().into())
        .collect();

    let label = req.label.trim();
    if label.is_empty() {
        return Err(ApiError::BadRequest("label is required".to_string()));
    }

    let (ccr, state_) = webauthn
        .start_passkey_registration(
            user.0.id,
            &user.0.username,
            &user.0.full_name,
            if exclude.is_empty() {
                None
            } else {
                Some(exclude)
            },
        )
        .map_err(|e| {
            tracing::error!("WebAuthn start_passkey_registration: {e}");
            ApiError::InternalServerError("WebAuthn ceremony failed".to_string())
        })?;

    let token = state.mfa_service.put_registration(WebauthnRegistration {
        user_id: user.0.id,
        label: label.to_string(),
        state: state_,
    });

    let options = serde_json::to_value(&ccr)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to serialize CCR: {e}")))?;

    Ok(Json(ApiResponse::success(WebauthnRegisterBeginResponse {
        challenge_token: token,
        options,
    })))
}

async fn webauthn_register_finish(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<WebauthnRegisterFinishRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let webauthn = state
        .mfa_service
        .webauthn()
        .ok_or_else(|| ApiError::Forbidden("WebAuthn disabled".to_string()))?;
    let pending = state
        .mfa_service
        .take_registration(&req.challenge_token)
        .ok_or_else(|| ApiError::BadRequest("Unknown or expired challenge_token".to_string()))?;
    if pending.user_id != user.0.id {
        return Err(ApiError::Forbidden(
            "challenge_token does not belong to this user".to_string(),
        ));
    }

    let credential: RegisterPublicKeyCredential = serde_json::from_value(req.response)
        .map_err(|e| ApiError::BadRequest(format!("Invalid WebAuthn response: {e}")))?;

    let passkey = webauthn
        .finish_passkey_registration(&credential, &pending.state)
        .map_err(|e| {
            tracing::warn!("WebAuthn finish_passkey_registration failed: {e}");
            ApiError::BadRequest(format!("WebAuthn registration rejected: {e}"))
        })?;

    let cred_id_bytes: Vec<u8> = passkey.cred_id().as_ref().to_vec();
    let stored = state.db.insert_user_webauthn(&NewUserMfaWebauthn {
        user_id: user.0.id,
        credential_id: cred_id_bytes,
        passkey: serde_json::to_value(&passkey)
            .map_err(|e| ApiError::InternalServerError(format!("serialize passkey: {e}")))?,
        label: pending.label,
    })?;

    state.db.recompute_user_mfa_enrolled(user.0.id)?;
    audit(
        &state,
        AuditEventType::MfaWebauthnRegistered,
        user.0.id,
        serde_json::json!({ "credential_row_id": stored.id, "label": stored.label }),
    );

    Ok(Json(ApiResponse::success(WebauthnRegisterFinishResponse {
        credential_id: stored.id,
    })))
}

async fn webauthn_remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let deleted = state.db.delete_user_webauthn(user.0.id, id)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Credential not found".to_string()));
    }
    state.db.recompute_user_mfa_enrolled(user.0.id)?;
    audit(
        &state,
        AuditEventType::MfaWebauthnRemoved,
        user.0.id,
        serde_json::json!({ "credential_row_id": id }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Credential removed".to_string()),
        error: None,
    }))
}

// --- Recovery codes -------------------------------------------------------

async fn recovery_regenerate(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let count = state.mfa_service.config().recovery_code_count;
    let codes = generate_recovery_codes(count);
    let hashes: Vec<String> = codes
        .iter()
        .map(|c| hash_recovery_code(c))
        .collect::<Result<_, _>>()
        .map_err(ApiError::InternalServerError)?;
    state.db.replace_user_recovery_codes(user.0.id, hashes)?;
    audit(
        &state,
        AuditEventType::MfaRecoveryCodesRegenerated,
        user.0.id,
        serde_json::json!({ "count": count }),
    );
    Ok(Json(ApiResponse::success(MfaRecoveryCodesResponse {
        recovery_codes: codes,
    })))
}

// ---------------------------------------------------------------------------
// /verify — completes a pending login challenge
// ---------------------------------------------------------------------------

async fn verify_login(
    State(state): State<AppState>,
    Json(req): Json<MfaVerifyRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let challenge = state
        .mfa_service
        .take_login(&req.challenge_token)
        .ok_or_else(|| ApiError::Unauthorized("Unknown or expired challenge_token".to_string()))?;

    let user = state
        .db
        .find_user_by_id(challenge.user_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::Unauthorized("User no longer exists".to_string()))?;

    let outcome = match req.method.as_str() {
        methods::TOTP => verify_totp_path(&state, &user, req.code.as_deref()),
        methods::WEBAUTHN => verify_webauthn_path(&state, &user, &challenge, req.response).await,
        methods::RECOVERY => verify_recovery_path(&state, &user, req.code.as_deref()),
        other => Err(ApiError::BadRequest(format!("Unknown method: {other}"))),
    };

    match outcome {
        Ok(()) => {
            audit(
                &state,
                AuditEventType::MfaLoginPassed,
                user.id,
                serde_json::json!({ "method": req.method }),
            );
            let resp = issue_token(&state, &user)?;
            Ok((
                StatusCode::OK,
                Json(ApiResponse::success_with_message(
                    resp,
                    "Login successful".to_string(),
                )),
            )
                .into_response())
        }
        Err(e) => {
            audit(
                &state,
                AuditEventType::MfaLoginFailed,
                user.id,
                serde_json::json!({ "method": req.method, "error": e.to_string() }),
            );
            Err(e)
        }
    }
}

fn verify_totp_path(
    state: &AppState,
    user: &crate::models::User,
    code: Option<&str>,
) -> Result<(), ApiError> {
    let code = code
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| ApiError::BadRequest("code is required for TOTP".to_string()))?;
    let stored = state
        .db
        .get_user_totp(user.id)
        .map_err(ApiError::from)?
        .filter(|t| t.confirmed_at.is_some())
        .ok_or_else(|| ApiError::Unauthorized("No confirmed TOTP for user".to_string()))?;
    if state
        .mfa_service
        .verify_totp(&stored.secret_base32, &user.email, code)
    {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("Invalid TOTP code".to_string()))
    }
}

async fn verify_webauthn_path(
    state: &AppState,
    user: &crate::models::User,
    challenge: &LoginChallenge,
    response: Option<serde_json::Value>,
) -> Result<(), ApiError> {
    let webauthn = state
        .mfa_service
        .webauthn()
        .ok_or_else(|| ApiError::Forbidden("WebAuthn disabled".to_string()))?;
    let auth_state = challenge
        .webauthn_auth
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("This challenge has no WebAuthn state".to_string()))?;
    let cred: PublicKeyCredential = serde_json::from_value(
        response.ok_or_else(|| ApiError::BadRequest("response is required".to_string()))?,
    )
    .map_err(|e| ApiError::BadRequest(format!("Invalid WebAuthn response: {e}")))?;

    let result = webauthn
        .finish_passkey_authentication(&cred, auth_state)
        .map_err(|e| {
            tracing::warn!("WebAuthn finish_passkey_authentication: {e}");
            ApiError::Unauthorized("WebAuthn authentication rejected".to_string())
        })?;

    // Touch last_used_at on the credential that just signed in.
    let used_id: Vec<u8> = result.cred_id().as_ref().to_vec();
    if let Err(e) = state.db.touch_user_webauthn_last_used(user.id, &used_id) {
        tracing::warn!("Failed to update webauthn last_used_at: {e}");
    }
    Ok(())
}

fn verify_recovery_path(
    state: &AppState,
    user: &crate::models::User,
    code: Option<&str>,
) -> Result<(), ApiError> {
    let code = code
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| ApiError::BadRequest("code is required for recovery".to_string()))?;
    let codes = state
        .db
        .list_unused_recovery_codes(user.id)
        .map_err(ApiError::from)?;
    for row in codes {
        if verify_recovery_code(code, &row.code_hash) {
            state
                .db
                .mark_recovery_code_used(row.id)
                .map_err(ApiError::from)?;
            audit(
                state,
                AuditEventType::MfaRecoveryCodeUsed,
                user.id,
                serde_json::json!({ "code_id": row.id }),
            );
            return Ok(());
        }
    }
    Err(ApiError::Unauthorized("Invalid recovery code".to_string()))
}

// ---------------------------------------------------------------------------
// Helper used by /api/auth/login when a user has MFA enrolled.
// ---------------------------------------------------------------------------

/// Build an MFA challenge for a freshly-authenticated user. Returns the
/// challenge JSON to embed in the login response. Used by `api/auth.rs`.
pub fn build_login_challenge(
    state: &AppState,
    user: &crate::models::User,
) -> Result<serde_json::Value, ApiError> {
    let mut methods_v: Vec<&'static str> = Vec::new();
    if state
        .db
        .get_user_totp(user.id)?
        .and_then(|t| t.confirmed_at)
        .is_some()
    {
        methods_v.push(methods::TOTP);
    }

    let webauthn_creds = state.db.list_user_webauthn(user.id)?;
    let mut webauthn_options: Option<serde_json::Value> = None;
    let mut webauthn_state: Option<PasskeyAuthentication> = None;
    if !webauthn_creds.is_empty() {
        if let Some(webauthn) = state.mfa_service.webauthn() {
            // Deserialize stored passkeys for the ceremony.
            let passkeys: Vec<Passkey> = webauthn_creds
                .iter()
                .filter_map(|row| serde_json::from_value::<Passkey>(row.passkey.clone()).ok())
                .collect();
            if !passkeys.is_empty() {
                match webauthn.start_passkey_authentication(&passkeys) {
                    Ok((rcr, st)) => {
                        webauthn_options = Some(serde_json::to_value(&rcr).map_err(|e| {
                            ApiError::InternalServerError(format!("Failed to serialize RCR: {e}"))
                        })?);
                        webauthn_state = Some(st);
                        methods_v.push(methods::WEBAUTHN);
                    }
                    Err(e) => tracing::warn!("WebAuthn start_passkey_authentication failed: {e}"),
                }
            }
        }
    }

    if state.db.count_unused_recovery_codes(user.id)? > 0 {
        methods_v.push(methods::RECOVERY);
    }

    if methods_v.is_empty() {
        // User has mfa_enrolled_at but no usable methods anymore. Treat as
        // not-enrolled to avoid permanent lockout.
        return Err(ApiError::InternalServerError(
            "User is marked MFA-enrolled but has no usable methods".to_string(),
        ));
    }

    let token = state.mfa_service.put_login(LoginChallenge {
        user_id: user.id,
        methods: methods_v.clone(),
        webauthn_auth: webauthn_state,
    });

    Ok(serde_json::json!({
        "mfa_required": true,
        "challenge_token": token,
        "methods": methods_v,
        "webauthn_options": webauthn_options,
    }))
}
