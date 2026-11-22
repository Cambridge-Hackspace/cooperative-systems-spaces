//! CS:S as an OpenID Connect provider (#225): the protocol endpoints, and the
//! administration of the apps that may use them.
//!
//! - `GET  /.well-known/openid-configuration` (mounted at the root by main.rs)
//! - `GET  /api/oauth/jwks`
//! - `GET  /api/oauth/authorize`               code flow, PKCE (S256) required
//! - `GET  /api/oauth/requests/{id}`           what the consent page shows
//! - `POST /api/oauth/requests/{id}/decision`  the member's answer
//! - `POST /api/oauth/token`                   code, refresh, client credentials
//! - `GET|POST /api/oauth/userinfo`
//! - `GET  /api/oauth/logout`                  RP-initiated logout
//! - `POST /api/oauth/end-session`             finishes it, from CS:S's own page
//! - `GET  /api/oauth/me/tools`                training, under `training:read`
//! - `GET  /api/oauth/members/{sub}/tools`     any member's, to an app with
//!   `training:members` (client credentials)
//!
//! **Why authorize doesn't decide.** CS:S's session cookie is `SameSite=Strict`
//! (its defense against cross-site request forgery), so a browser arriving from
//! another app -- a cross-site navigation -- does not send it. `authorize`
//! therefore never sees who is signed in. It checks the request, parks it, and
//! sends the browser to CS:S's own page (`/oauth/consent/{id}`), whose requests
//! are same-site and do carry the session. That page signs the member in if
//! needed and asks `decision`, which issues the code. Logout works the same way.
//! The cookie keeps its protection; nothing here loosens it.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::responses::ApiResponse;
use crate::auth::{AuthService, AuthUser, PasswordHashUtil};
use crate::models::{
    AuditEventType, NewAuditLog, NewOauthAuthorizationRequest, NewOauthClient,
    NewOauthRefreshToken, OauthAuthorizationCode, OauthClient, User,
};
use crate::oidc::{self, flow, store};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/jwks", get(jwks))
        .route("/authorize", get(authorize))
        .route("/requests/{id}", get(request_info))
        .route("/requests/{id}/decision", post(decide))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo).post(userinfo))
        .route("/logout", get(logout))
        .route("/end-session", post(end_session))
        .route("/me/tools", get(my_tools))
        .route("/members/{sub}/tools", get(member_tools))
}

/// Mounted at `/api/admin/oauth`. Each handler checks `oauth.clients.manage`.
pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/clients", get(list_clients).post(create_client))
        .route(
            "/clients/{id}",
            get(get_client).patch(update_client).delete(delete_client),
        )
        .route("/clients/{id}/secret", post(rotate_secret))
        .route("/keys/rotate", post(rotate_keys))
}

fn disabled() -> ApiError {
    ApiError::NotFound("The OpenID Connect provider is not enabled".to_string())
}

fn audit(state: &AppState, event: AuditEventType, user: Option<Uuid>, data: Value) {
    let _ = state.db.create_audit_log(&NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: user,
        actor_id: user,
        event_data: data,
        ip_address: None,
        user_agent: None,
    });
}

// ---------------------------------------------------------------------------
// Discovery and keys
// ---------------------------------------------------------------------------

/// `GET /.well-known/openid-configuration`
pub async fn discovery(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    let iss = oidc::issuer(&state);
    let scopes: Vec<&str> = flow::SCOPES.iter().map(|(s, _)| *s).collect();
    Ok(Json(json!({
        "issuer": iss,
        "authorization_endpoint": format!("{iss}/api/oauth/authorize"),
        "token_endpoint": format!("{iss}/api/oauth/token"),
        "userinfo_endpoint": format!("{iss}/api/oauth/userinfo"),
        "jwks_uri": format!("{iss}/api/oauth/jwks"),
        "end_session_endpoint": format!("{iss}/api/oauth/logout"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token", "client_credentials", oidc::card::GRANT],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": [oidc::keys::ALG],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post", "none"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": scopes,
        "claims_supported": [
            "sub", "iss", "aud", "exp", "iat", "auth_time", "nonce", "azp",
            "name", "preferred_username", "email", "email_verified", "roles"
        ],
    })))
}

async fn jwks(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    Ok(Json(json!({ "keys": oidc::published_jwks(&state)? })))
}

// ---------------------------------------------------------------------------
// Authorization
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AuthorizeQuery {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    prompt: Option<String>,
}

/// An error shown on CS:S itself: the redirect URI can't be trusted, so the
/// browser is not sent anywhere (RFC 6749 section 4.1.2.1).
fn refuse_here(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "success": false, "error": "invalid_request", "error_description": msg })),
    )
        .into_response()
}

/// An error sent back to the app, at its registered redirect URI.
fn refuse_to_client(redirect_uri: &str, error: &str, desc: &str, state: Option<&str>) -> Response {
    let mut params = vec![("error", error), ("error_description", desc)];
    if let Some(s) = state {
        params.push(("state", s));
    }
    match flow::redirect_with(redirect_uri, &params) {
        Some(url) => Redirect::to(&url).into_response(),
        None => refuse_here(desc),
    }
}

/// `GET /api/oauth/authorize`
async fn authorize(
    State(state): State<AppState>,
    Query(q): Query<AuthorizeQuery>,
) -> Result<Response, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    // The client and its redirect URI come first: until both are known good,
    // nothing may be sent to the redirect URI.
    let Some(client_id) = q.client_id.as_deref() else {
        return Ok(refuse_here("client_id is required"));
    };
    let Some(client) = state.db.oauth_client_by_client_id(client_id)? else {
        return Ok(refuse_here("unknown client_id"));
    };
    if client.disabled {
        return Ok(refuse_here("this app is disabled"));
    }
    let Some(redirect_uri) = q.redirect_uri.as_deref() else {
        return Ok(refuse_here("redirect_uri is required"));
    };
    if !flow::redirect_allowed(&client.redirect_uris, redirect_uri) {
        return Ok(refuse_here("redirect_uri is not registered for this app"));
    }
    let st = q.state.as_deref();
    if q.response_type.as_deref() != Some("code") {
        return Ok(refuse_to_client(
            redirect_uri,
            "unsupported_response_type",
            "only the authorization code flow is supported",
            st,
        ));
    }
    // PKCE is required of every app, confidential or not (OAuth 2.0 Security
    // BCP), and only S256: `plain` sends the verifier through the browser.
    let challenge = q.code_challenge.as_deref().unwrap_or("");
    if q.code_challenge_method.as_deref() != Some("S256")
        || !flow::challenge_is_well_formed(challenge)
    {
        return Ok(refuse_to_client(
            redirect_uri,
            "invalid_request",
            "PKCE is required: code_challenge with code_challenge_method=S256",
            st,
        ));
    }
    let scopes = flow::parse_scopes(q.scope.as_deref().unwrap_or(""));
    if !scopes.contains("openid") {
        return Ok(refuse_to_client(
            redirect_uri,
            "invalid_scope",
            "the openid scope is required",
            st,
        ));
    }
    if let Some(bad) = scopes.iter().find(|s| {
        !flow::scope_is_known(s)
            || flow::APP_SCOPES.contains(&s.as_str())
            || !client.allowed_scopes.contains(*s)
    }) {
        return Ok(refuse_to_client(
            redirect_uri,
            "invalid_scope",
            &format!("this app may not ask for {bad}"),
            st,
        ));
    }
    let config = state.config_manager.get_config();
    let request = state
        .db
        .oauth_insert_request(&NewOauthAuthorizationRequest {
            client_id: client.id,
            redirect_uri: redirect_uri.to_string(),
            scope: flow::join_scopes(&scopes),
            state: q.state.clone(),
            nonce: q.nonce.clone(),
            code_challenge: challenge.to_string(),
            prompt: q.prompt.clone(),
            expires_at: Utc::now() + Duration::seconds(config.oidc.request_ttl_secs),
        })?;
    Ok(Redirect::to(&format!("/oauth/consent/{}", request.id)).into_response())
}

#[derive(Debug, Serialize)]
struct ScopeView {
    scope: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct RequestView {
    app: String,
    /// Where the member will be sent back to, for them to recognize.
    returns_to: String,
    scopes: Vec<ScopeView>,
    /// No question to ask: the app skips consent, or the member already agreed
    /// to all of this. The page finishes without showing anything.
    automatic: bool,
}

/// `GET /api/oauth/requests/{id}` -- what CS:S's consent page shows.
async fn request_info(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RequestView>>, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    let Some(req) = state.db.oauth_live_request(id)? else {
        return Err(ApiError::NotFound(
            "This sign-in request has expired. Start again from the app.".to_string(),
        ));
    };
    let Some(client) = state.db.oauth_get_client(req.client_id)? else {
        return Err(ApiError::NotFound(
            "The app is no longer registered".to_string(),
        ));
    };
    let scopes: Vec<String> = flow::parse_scopes(&req.scope).into_iter().collect();
    let automatic = needs_no_question(&state, &user.0, &client, &scopes, req.prompt.as_deref())?;
    Ok(Json(ApiResponse::success(RequestView {
        app: client.name,
        returns_to: url::Url::parse(&req.redirect_uri)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default(),
        scopes: scopes
            .iter()
            .map(|s| ScopeView {
                scope: s.clone(),
                description: flow::SCOPES
                    .iter()
                    .find(|(k, _)| k == s)
                    .map(|(_, d)| d.to_string())
                    .unwrap_or_default(),
            })
            .collect(),
        automatic,
    })))
}

fn needs_no_question(
    state: &AppState,
    user: &User,
    client: &OauthClient,
    scopes: &[String],
    prompt: Option<&str>,
) -> Result<bool, ApiError> {
    if prompt == Some("consent") {
        return Ok(false);
    }
    if client.skip_consent {
        return Ok(true);
    }
    Ok(state
        .db
        .oauth_consent(user.id, client.id)?
        .is_some_and(|c| scopes.iter().all(|s| c.scopes.contains(s))))
}

#[derive(Debug, Deserialize)]
struct Decision {
    approve: bool,
}

#[derive(Debug, Serialize)]
struct DecisionView {
    /// Where the browser goes next: the app's redirect URI with a code, or
    /// with `error=access_denied`.
    redirect: String,
}

/// `POST /api/oauth/requests/{id}/decision` -- the member's answer. Called by
/// CS:S's own page, same-site, so the session is present.
async fn decide(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(d): Json<Decision>,
) -> Result<Json<ApiResponse<DecisionView>>, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    let Some(req) = state.db.oauth_live_request(id)? else {
        return Err(ApiError::NotFound(
            "This sign-in request has expired. Start again from the app.".to_string(),
        ));
    };
    // Answered once: a second answer finds nothing.
    if !state.db.oauth_delete_request(id)? {
        return Err(ApiError::NotFound(
            "This sign-in request was already answered".to_string(),
        ));
    }
    let Some(client) = state.db.oauth_get_client(req.client_id)? else {
        return Err(ApiError::NotFound(
            "The app is no longer registered".to_string(),
        ));
    };
    let st = req.state.as_deref();
    let back = |params: &[(&str, &str)]| {
        let mut p = params.to_vec();
        if let Some(s) = st {
            p.push(("state", s));
        }
        flow::redirect_with(&req.redirect_uri, &p).ok_or_else(|| {
            ApiError::InternalServerError("registered redirect_uri no longer parses".to_string())
        })
    };
    if client.disabled {
        return Ok(Json(ApiResponse::success(DecisionView {
            redirect: back(&[
                ("error", "access_denied"),
                ("error_description", "this app is disabled"),
            ])?,
        })));
    }
    let scopes: Vec<String> = flow::parse_scopes(&req.scope).into_iter().collect();
    let automatic = needs_no_question(&state, &user.0, &client, &scopes, req.prompt.as_deref())?;
    if !d.approve && !automatic {
        return Ok(Json(ApiResponse::success(DecisionView {
            redirect: back(&[
                ("error", "access_denied"),
                ("error_description", "the member declined"),
            ])?,
        })));
    }
    if !automatic {
        state
            .db
            .oauth_grant_consent(user.0.id, client.id, &scopes)?;
        audit(
            &state,
            AuditEventType::OauthConsentGranted,
            Some(user.0.id),
            json!({ "client_id": client.client_id, "app": client.name, "scopes": scopes }),
        );
    }
    let config = state.config_manager.get_config();
    let code = flow::new_secret();
    let now = Utc::now();
    state.db.oauth_insert_code(&OauthAuthorizationCode {
        code_hash: flow::hash_secret(&code),
        client_id: client.id,
        user_id: user.0.id,
        redirect_uri: req.redirect_uri.clone(),
        scope: req.scope.clone(),
        nonce: req.nonce.clone(),
        code_challenge: req.code_challenge.clone(),
        auth_time: session_auth_time(&state, &headers).unwrap_or(now),
        created_at: now,
        expires_at: now + Duration::seconds(config.oidc.code_ttl_secs),
        used_at: None,
    })?;
    Ok(Json(ApiResponse::success(DecisionView {
        redirect: back(&[("code", code.as_str())])?,
    })))
}

/// When the member signed in to CS:S: the session token's `iat`.
fn session_auth_time(state: &AppState, headers: &HeaderMap) -> Option<DateTime<Utc>> {
    let token = crate::auth::select_request_token(
        headers.get(header::COOKIE).and_then(|v| v.to_str().ok()),
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
    )
    .ok()?;
    let config = state.config_manager.get_config();
    let claims = AuthService::new(&state.db, &config.auth.jwt_secret)
        .verify_token(token)
        .ok()?;
    DateTime::from_timestamp(claims.iat as i64, 0)
}

// ---------------------------------------------------------------------------
// The token endpoint, in RFC 6749's own error format
// ---------------------------------------------------------------------------

/// An error in the shape OAuth clients expect: `{ error, error_description }`.
struct OauthError {
    status: StatusCode,
    error: &'static str,
    description: String,
    /// Basic credentials were offered and refused: say how to authenticate.
    challenge: bool,
}

impl OauthError {
    fn new(error: &'static str, description: impl Into<String>) -> Self {
        OauthError {
            status: StatusCode::BAD_REQUEST,
            error,
            description: description.into(),
            challenge: false,
        }
    }
}

impl IntoResponse for OauthError {
    fn into_response(self) -> Response {
        let mut resp = (
            self.status,
            // RFC 6749's shape, plus CS:S's `success: false`, which OAuth
            // clients ignore and CS:S's own clients look for.
            Json(json!({ "success": false, "error": self.error, "error_description": self.description })),
        )
            .into_response();
        no_store(&mut resp);
        if self.challenge {
            resp.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"CS:S\""),
            );
        }
        resp
    }
}

impl From<ApiError> for OauthError {
    fn from(e: ApiError) -> Self {
        tracing::error!("OIDC token endpoint failed: {e:?}");
        OauthError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "server_error",
            description: "the server could not complete the request".to_string(),
            challenge: false,
        }
    }
}

impl From<crate::database::DatabaseError> for OauthError {
    fn from(e: crate::database::DatabaseError) -> Self {
        ApiError::from(e).into()
    }
}

fn no_store(resp: &mut Response) {
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

#[derive(Debug, Deserialize)]
struct TokenForm {
    grant_type: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    scope: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    /// A card sign-in's second factor (#241): the ticket from the scan reply,
    /// and the member's TOTP code.
    ticket: Option<String>,
    totp: Option<String>,
}

/// Who is calling: `client_secret_basic`, `client_secret_post`, or a public
/// client naming itself (`none`), which must then prove itself with PKCE.
fn authenticate_client(
    state: &AppState,
    headers: &HeaderMap,
    form: &TokenForm,
) -> Result<OauthClient, OauthError> {
    let basic = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| {
            base64::engine::general_purpose::STANDARD
                .decode(b.trim())
                .ok()
        })
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| {
            s.split_once(':').map(|(id, secret)| {
                // RFC 6749 section 2.3.1: both halves are form-urlencoded.
                let dec = |x: &str| {
                    url::form_urlencoded::parse(format!("x={x}").as_bytes())
                        .next()
                        .map(|(_, v)| v.into_owned())
                        .unwrap_or_default()
                };
                (dec(id), dec(secret))
            })
        });
    let used_basic = basic.is_some();
    let (client_id, secret) = match basic {
        Some((id, secret)) => (id, Some(secret)),
        None => (
            form.client_id.clone().unwrap_or_default(),
            form.client_secret.clone(),
        ),
    };
    let refused = |what: &str| OauthError {
        status: if used_basic {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::BAD_REQUEST
        },
        error: "invalid_client",
        description: what.to_string(),
        challenge: used_basic,
    };
    if client_id.is_empty() {
        return Err(refused("client authentication is required"));
    }
    let Some(client) = state.db.oauth_client_by_client_id(&client_id)? else {
        // Hash anyway, so an unknown id takes as long as a wrong secret.
        let _ = PasswordHashUtil::verify(secret.as_deref().unwrap_or(""), &dummy_hash());
        return Err(refused("unknown client or wrong secret"));
    };
    if client.disabled {
        return Err(refused("this app is disabled"));
    }
    match (&client.secret_hash, secret) {
        (Some(hash), Some(secret)) => {
            if !PasswordHashUtil::verify(&secret, hash).unwrap_or(false) {
                return Err(refused("unknown client or wrong secret"));
            }
        }
        (Some(_), None) => return Err(refused("this app must authenticate with its secret")),
        (None, Some(_)) => return Err(refused("this app has no secret")),
        (None, None) => {}
    }
    Ok(client)
}

fn dummy_hash() -> String {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| PasswordHashUtil::hash("oidc-timing-equalization").unwrap_or_default())
        .clone()
}

/// `POST /api/oauth/token` (form-encoded)
async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    match token_inner(&state, &headers, &body).await {
        Ok(v) => {
            let mut resp = Json(v).into_response();
            no_store(&mut resp);
            resp
        }
        Err(e) => e.into_response(),
    }
}

async fn token_inner(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Value, OauthError> {
    if !oidc::enabled(state) {
        return Err(OauthError {
            status: StatusCode::NOT_FOUND,
            ..OauthError::new(
                "invalid_request",
                "the OpenID Connect provider is not enabled",
            )
        });
    }
    let form: TokenForm = serde_urlencoded::from_bytes(body).map_err(|_| {
        OauthError::new(
            "invalid_request",
            "the body must be application/x-www-form-urlencoded",
        )
    })?;
    let client = authenticate_client(state, headers, &form)?;
    match form.grant_type.as_deref() {
        Some("authorization_code") => code_grant(state, &client, &form),
        Some("refresh_token") => refresh_grant(state, &client, &form),
        Some("client_credentials") => client_credentials_grant(state, &client, &form),
        Some(oidc::card::GRANT) => card_grant(state, &client, &form),
        Some(_) => Err(OauthError::new(
            "unsupported_grant_type",
            "unsupported grant_type",
        )),
        None => Err(OauthError::new("invalid_request", "grant_type is required")),
    }
}

fn code_grant(
    state: &AppState,
    client: &OauthClient,
    form: &TokenForm,
) -> Result<Value, OauthError> {
    let invalid = |why: &str| OauthError::new("invalid_grant", why);
    let code = form
        .code
        .as_deref()
        .ok_or_else(|| OauthError::new("invalid_request", "code is required"))?;
    let code = match state
        .db
        .oauth_redeem_code(&flow::hash_secret(code), client.id)?
    {
        store::Redeemed::Fresh(c) => c,
        store::Redeemed::Replayed => {
            tracing::warn!(
                "authorization code replayed by {}; its tokens are revoked",
                client.client_id
            );
            return Err(invalid("this code was already used"));
        }
        store::Redeemed::Unknown => return Err(invalid("unknown code")),
    };
    if code.expires_at <= Utc::now() {
        return Err(invalid("this code has expired"));
    }
    if form.redirect_uri.as_deref() != Some(code.redirect_uri.as_str()) {
        return Err(invalid(
            "redirect_uri does not match the one the code was issued for",
        ));
    }
    let verifier = form.code_verifier.as_deref().unwrap_or("");
    if !flow::pkce_matches(verifier, &code.code_challenge) {
        return Err(invalid("code_verifier does not match the code_challenge"));
    }
    let user = live_user(state, code.user_id)?;
    let scopes: Vec<String> = flow::parse_scopes(&code.scope).into_iter().collect();
    let tokens = issue_user_tokens(
        state,
        client,
        &user,
        &scopes,
        code.nonce.as_deref(),
        code.auth_time,
        Some(code.code_hash.as_str()),
        None,
    )?;
    audit(
        state,
        AuditEventType::OauthSignin,
        Some(user.id),
        json!({ "client_id": client.client_id, "app": client.name, "scopes": scopes }),
    );
    Ok(tokens)
}

/// A card tap's sign-in (#241): `code` from the scan reply, or `ticket` and
/// `totp` where the resource asked for a second factor. Only for the app the
/// sign-in was made for, and only one with a secret: a public client naming
/// itself is never given a card sign-in.
fn card_grant(
    state: &AppState,
    client: &OauthClient,
    form: &TokenForm,
) -> Result<Value, OauthError> {
    let invalid = |m: &str| OauthError::new("invalid_grant", m);
    if !client.confidential {
        return Err(OauthError::new(
            "unauthorized_client",
            "only an app with a secret may complete a card sign-in",
        ));
    }
    let code = match (
        form.code.as_deref(),
        form.ticket.as_deref(),
        form.totp.as_deref(),
    ) {
        (Some(code), None, None) => code.to_string(),
        (None, Some(ticket), Some(totp)) => {
            match oidc::card::second_factor(state, client, ticket, totp)? {
                Ok(code) => code,
                Err(oidc::card::Refused::WrongCode(user)) => {
                    audit(
                        state,
                        AuditEventType::CardSignInSecondFactorFailed,
                        Some(user),
                        json!({ "client_id": client.client_id, "app": client.name }),
                    );
                    return Err(invalid("that code is not right"));
                }
                Err(oidc::card::Refused::TooMany) => {
                    return Err(invalid("too many tries: wait a while, then tap again"));
                }
                Err(oidc::card::Refused::Ticket) => {
                    return Err(invalid("this sign-in has expired: tap again"));
                }
            }
        }
        _ => {
            return Err(OauthError::new(
                "invalid_request",
                "a card sign-in takes code, or ticket and totp",
            ))
        }
    };
    let row = match state
        .db
        .card_sign_in_redeem(&flow::hash_secret(&code), client.id)?
    {
        store::CardRedeemed::Fresh(r) => r,
        store::CardRedeemed::Replayed => {
            tracing::warn!(
                "card sign-in code replayed by {}; its tokens are revoked",
                client.client_id
            );
            return Err(invalid("this sign-in was already used"));
        }
        store::CardRedeemed::Unknown => return Err(invalid("unknown card sign-in")),
    };
    if row.expires_at <= Utc::now() {
        return Err(invalid("this sign-in has expired: tap again"));
    }
    let user = live_user(state, row.user_id)?;
    // The app's member scopes, never one that acts as the app itself.
    let scopes: Vec<String> = client
        .allowed_scopes
        .iter()
        .filter(|s| !flow::APP_SCOPES.contains(&s.as_str()))
        .cloned()
        .collect();
    let amr: Vec<&str> = row.amr.split(' ').collect();
    let tokens = issue_user_tokens(
        state,
        client,
        &user,
        &scopes,
        None,
        row.created_at,
        row.code_hash.as_deref(),
        Some(&amr),
    )?;
    audit(
        state,
        AuditEventType::CardSignIn,
        Some(user.id),
        json!({
            "client_id": client.client_id, "app": client.name, "scopes": scopes,
            "resource_id": row.resource_id, "amr": amr,
        }),
    );
    Ok(tokens)
}

fn refresh_grant(
    state: &AppState,
    client: &OauthClient,
    form: &TokenForm,
) -> Result<Value, OauthError> {
    let presented = form
        .refresh_token
        .as_deref()
        .ok_or_else(|| OauthError::new("invalid_request", "refresh_token is required"))?;
    let Some(old) = state
        .db
        .oauth_use_refresh_token(&flow::hash_secret(presented), client.id)?
    else {
        return Err(OauthError::new(
            "invalid_grant",
            "unknown, expired or already used refresh token",
        ));
    };
    // A refresh may narrow the scope, never widen it.
    let granted: Vec<String> = flow::parse_scopes(&old.scope).into_iter().collect();
    let scopes = match form.scope.as_deref() {
        Some(s) => {
            let asked: Vec<String> = flow::parse_scopes(s).into_iter().collect();
            if asked.iter().any(|a| !granted.contains(a)) {
                return Err(OauthError::new(
                    "invalid_scope",
                    "a refresh cannot add scopes",
                ));
            }
            asked
        }
        None => granted,
    };
    let user = live_user(state, old.user_id)?;
    issue_user_tokens(
        state,
        client,
        &user,
        &scopes,
        None,
        old.auth_time,
        old.code_hash.as_deref(),
        None,
    )
}

fn client_credentials_grant(
    state: &AppState,
    client: &OauthClient,
    form: &TokenForm,
) -> Result<Value, OauthError> {
    if !client.confidential {
        return Err(OauthError::new(
            "unauthorized_client",
            "only an app with a secret may act as itself",
        ));
    }
    let asked: Vec<String> = flow::parse_scopes(form.scope.as_deref().unwrap_or(""))
        .into_iter()
        .collect();
    if asked.is_empty() {
        return Err(OauthError::new(
            "invalid_scope",
            "ask for the scopes this app acts with",
        ));
    }
    if let Some(bad) = asked
        .iter()
        .find(|s| !flow::APP_SCOPES.contains(&s.as_str()) || !client.allowed_scopes.contains(*s))
    {
        return Err(OauthError::new(
            "invalid_scope",
            format!("this app may not act with {bad}"),
        ));
    }
    let (access, ttl) = oidc::access_token(state, &client.client_id, None, &asked)?;
    Ok(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ttl,
        "scope": asked.join(" "),
    }))
}

/// The member a code or refresh token names, still active. A member who was
/// deactivated after signing in gets no new tokens.
fn live_user(state: &AppState, id: Uuid) -> Result<User, OauthError> {
    match state.db.find_user_by_id(id)? {
        Some(u) if u.is_active => Ok(u),
        _ => Err(OauthError::new(
            "invalid_grant",
            "the member is no longer active",
        )),
    }
}

fn issue_user_tokens(
    state: &AppState,
    client: &OauthClient,
    user: &User,
    scopes: &[String],
    nonce: Option<&str>,
    auth_time: DateTime<Utc>,
    code_hash: Option<&str>,
    amr: Option<&[&str]>,
) -> Result<Value, OauthError> {
    let config = state.config_manager.get_config();
    let now = Utc::now();
    let (access, ttl) = oidc::access_token(state, &client.client_id, Some(user.id), scopes)?;
    let mut id_claims = oidc::user_claims(state, user, scopes)?;
    id_claims["iss"] = json!(oidc::issuer(state));
    id_claims["aud"] = json!(client.client_id);
    id_claims["azp"] = json!(client.client_id);
    id_claims["iat"] = json!(now.timestamp());
    id_claims["exp"] = json!(now.timestamp() + config.oidc.id_token_ttl_secs);
    id_claims["auth_time"] = json!(auth_time.timestamp());
    if let Some(n) = nonce {
        id_claims["nonce"] = json!(n);
    }
    // How the member proved who they are, when it was not a password at CS:S
    // (#241): "hwk" for a card, and "otp" beside it for a TOTP code.
    if let Some(amr) = amr {
        id_claims["amr"] = json!(amr);
    }
    let id_token = oidc::sign(state, &id_claims)?;
    let refresh = flow::new_secret();
    state.db.oauth_insert_refresh_token(&NewOauthRefreshToken {
        token_hash: flow::hash_secret(&refresh),
        client_id: client.id,
        user_id: user.id,
        scope: scopes.join(" "),
        auth_time,
        code_hash: code_hash.map(str::to_string),
        expires_at: now + Duration::days(config.oidc.refresh_token_ttl_days),
    })?;
    Ok(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ttl,
        "id_token": id_token,
        "refresh_token": refresh,
        "scope": scopes.join(" "),
    }))
}

// ---------------------------------------------------------------------------
// Bearer access tokens: userinfo, and what an app may read with them
// ---------------------------------------------------------------------------

/// A member's access token, checked. Its shape is checked before anything
/// touches the database, so a request with no token or a malformed one is a
/// 401 even when the database is down.
struct MemberToken {
    user: User,
    scopes: Vec<String>,
}

fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    let unauthorized = || ApiError::Unauthorized("A bearer access token is required".to_string());
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or_else(unauthorized)?
        .trim();
    if token.is_empty() {
        return Err(unauthorized());
    }
    Ok(token)
}

fn member_token(
    state: &AppState,
    headers: &HeaderMap,
    scope: &str,
) -> Result<MemberToken, ApiError> {
    let token = bearer(headers)?;
    if !oidc::enabled(state) {
        return Err(disabled());
    }
    let claims =
        oidc::verify(state, token, Some(&oidc::api_audience(state)), false).map_err(|why| {
            tracing::info!("access token refused: {why:?}");
            ApiError::Unauthorized("The access token is not valid".to_string())
        })?;
    let scopes: Vec<String> = claims
        .get("scope")
        .and_then(Value::as_str)
        .map(|s| flow::parse_scopes(s).into_iter().collect())
        .unwrap_or_default();
    if !scopes.iter().any(|s| s == scope) {
        return Err(ApiError::Forbidden(format!(
            "This token does not carry the {scope} scope"
        )));
    }
    let user_id = claims
        .get("sub")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| ApiError::Unauthorized("This token is not a member's".to_string()))?;
    // The app that holds it must still be allowed to, and the member active.
    let client_id = claims
        .get("client_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match state.db.oauth_client_by_client_id(client_id)? {
        Some(c) if !c.disabled => {}
        _ => {
            return Err(ApiError::Unauthorized(
                "The app is no longer registered".to_string(),
            ))
        }
    }
    let user = match state.db.find_user_by_id(user_id)? {
        Some(u) if u.is_active => u,
        _ => {
            return Err(ApiError::Unauthorized(
                "The member is no longer active".to_string(),
            ))
        }
    };
    Ok(MemberToken { user, scopes })
}

/// `GET|POST /api/oauth/userinfo`
async fn userinfo(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let t = member_token(&state, &headers, "openid")?;
    Ok(Json(oidc::user_claims(&state, &t.user, &t.scopes)?))
}

#[derive(Debug, Serialize)]
struct ToolTraining {
    id: Uuid,
    external_id: Option<String>,
    name: String,
    /// The member may start this tool: trained on it, or it needs no training.
    authorized: bool,
}

/// `GET /api/oauth/me/tools` -- which tools the member may use, under the
/// `training:read` scope. Only the token's own member, never anyone else.
async fn my_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<ToolTraining>>>, ApiError> {
    let t = member_token(&state, &headers, "training:read")?;
    Ok(Json(ApiResponse::success(tools_for(&state, &t.user)?)))
}

/// `GET /api/oauth/members/{sub}/tools` -- the same answer for any member, to
/// an app acting as itself with `training:members` (client credentials). It
/// lets an app ask at each use, without keeping members' tokens. A member who
/// does not exist or is deactivated is 404: an app learns nothing it may use.
async fn member_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(sub): Path<String>,
) -> Result<Json<ApiResponse<Vec<ToolTraining>>>, ApiError> {
    app_from_token(&state, &headers, "training:members")?;
    let not_found = || ApiError::NotFound("No such member".to_string());
    let id = Uuid::parse_str(&sub).map_err(|_| not_found())?;
    let user = match state.db.find_user_by_id(id)? {
        Some(u) if u.is_active => u,
        _ => return Err(not_found()),
    };
    Ok(Json(ApiResponse::success(tools_for(&state, &user)?)))
}

/// Every tool, and whether `user` may start it: trained on it, or it needs no
/// training.
fn tools_for(state: &AppState, user: &User) -> Result<Vec<ToolTraining>, ApiError> {
    let mut out = Vec::new();
    for tool in state.db.oauth_all_tools()? {
        let authorized =
            state
                .db
                .user_is_authorized_for_tool(user.id, tool.id, tool.requires_training)?;
        out.push(ToolTraining {
            id: tool.id,
            external_id: tool.external_id.clone(),
            name: tool.name.clone(),
            authorized,
        });
    }
    Ok(out)
}

/// The app behind an app token: a client-credentials token that carries
/// `scope`, an app scope (`notify:members`, `training:members`), for an app
/// that may still hold it. The header's shape is checked before anything
/// touches the database.
pub(crate) fn app_from_token(
    state: &AppState,
    headers: &HeaderMap,
    scope: &str,
) -> Result<OauthClient, ApiError> {
    let unauthorized = || ApiError::Unauthorized("An app access token is required".to_string());
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(unauthorized)?;
    if !oidc::enabled(state) {
        return Err(disabled());
    }
    let claims =
        oidc::verify(state, token, Some(&oidc::api_audience(state)), false).map_err(|why| {
            tracing::info!("app token refused: {why:?}");
            unauthorized()
        })?;
    let sub = claims
        .get("sub")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(client_id) = sub.strip_prefix("client:") else {
        return Err(ApiError::Forbidden(
            "This is a member's token. This takes the app's own token (client credentials)."
                .to_string(),
        ));
    };
    let scopes = claims
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !scopes.split_whitespace().any(|s| s == scope) {
        return Err(ApiError::Forbidden(format!(
            "This token does not carry {scope}"
        )));
    }
    match state.db.oauth_client_by_client_id(client_id)? {
        // Checked again now: an app whose scope was taken away, or that was
        // disabled, stops being able to send at once, not when its token expires.
        Some(c) if !c.disabled && c.allowed_scopes.iter().any(|s| s == scope) => Ok(c),
        _ => Err(unauthorized()),
    }
}

// ---------------------------------------------------------------------------
// Logout
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
struct LogoutQuery {
    id_token_hint: Option<String>,
    post_logout_redirect_uri: Option<String>,
    state: Option<String>,
}

/// `GET /api/oauth/logout` -- RP-initiated logout. The browser arrives
/// cross-site, without the session, so it is sent to CS:S's own page with the
/// same parameters; that page calls `end-session`.
async fn logout(
    State(state): State<AppState>,
    Query(q): Query<LogoutQuery>,
) -> Result<Response, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    let query = serde_urlencoded::to_string(&q)
        .map_err(|e| ApiError::InternalServerError(format!("logout query: {e}")))?;
    Ok(Redirect::to(&format!("/oauth/logout?{query}")).into_response())
}

/// `POST /api/oauth/end-session` -- ends the CS:S session and says where to go.
/// The redirect is honored only when the hint names an app and the URI is
/// registered for that app; anything else goes to CS:S's home page. That is
/// what stops this being an open redirect.
async fn end_session(
    State(state): State<AppState>,
    Json(q): Json<LogoutQuery>,
) -> Result<Response, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    let mut redirect = "/".to_string();
    if let (Some(hint), Some(uri)) = (
        q.id_token_hint.as_deref(),
        q.post_logout_redirect_uri.as_deref(),
    ) {
        // An ID token may be offered after it has expired; it only has to be ours.
        if let Ok(claims) = oidc::verify(&state, hint, None, true) {
            let aud = claims
                .get("aud")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(client) = state.db.oauth_client_by_client_id(aud)? {
                if flow::redirect_allowed(&client.post_logout_redirect_uris, uri) {
                    let mut params = Vec::new();
                    if let Some(s) = q.state.as_deref() {
                        params.push(("state", s));
                    }
                    redirect = flow::redirect_with(uri, &params).unwrap_or(redirect);
                }
            }
        }
    }
    let config = state.config_manager.get_config();
    let mut resp = Json(ApiResponse::success(json!({ "redirect": redirect }))).into_response();
    crate::api::auth::attach_session_cookie(
        &mut resp,
        crate::auth::session_clear_cookie(config.auth.cookie_secure),
    );
    Ok(resp)
}

// ---------------------------------------------------------------------------
// Administration of apps
// ---------------------------------------------------------------------------

async fn manager(state: &AppState, user: &AuthUser) -> Result<(), ApiError> {
    if !state
        .db
        .user_has_permission(user.0.id, "oauth.clients.manage")?
    {
        return Err(ApiError::Forbidden(
            "Managing apps needs oauth.clients.manage".to_string(),
        ));
    }
    if !oidc::enabled(state) {
        return Err(disabled());
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ClientRequest {
    name: Option<String>,
    confidential: Option<bool>,
    redirect_uris: Option<Vec<String>>,
    post_logout_redirect_uris: Option<Vec<String>>,
    allowed_scopes: Option<Vec<String>>,
    skip_consent: Option<bool>,
    disabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct CreatedClient {
    client: OauthClient,
    /// Shown once. Only its hash is kept.
    client_secret: Option<String>,
}

fn check_uris(uris: &[String], what: &str) -> Result<(), ApiError> {
    if let Some(bad) = uris.iter().find(|u| !flow::redirect_is_registrable(u)) {
        return Err(ApiError::BadRequest(format!(
            "{what} must be absolute https URLs without a fragment (http only for localhost): {bad}"
        )));
    }
    Ok(())
}

fn check_scopes(scopes: &[String]) -> Result<(), ApiError> {
    if let Some(bad) = scopes.iter().find(|s| !flow::scope_is_known(s)) {
        return Err(ApiError::BadRequest(format!("unknown scope: {bad}")));
    }
    Ok(())
}

async fn list_clients(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<ApiResponse<Vec<OauthClient>>>, ApiError> {
    manager(&state, &user).await?;
    Ok(Json(ApiResponse::success(state.db.oauth_list_clients()?)))
}

async fn get_client(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<OauthClient>>, ApiError> {
    manager(&state, &user).await?;
    let Some(c) = state.db.oauth_get_client(id)? else {
        return Err(ApiError::NotFound("App not found".to_string()));
    };
    Ok(Json(ApiResponse::success(c)))
}

async fn create_client(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ClientRequest>,
) -> Result<(StatusCode, Json<ApiResponse<CreatedClient>>), ApiError> {
    manager(&state, &user).await?;
    let name = req.name.as_deref().map(str::trim).unwrap_or("");
    if name.is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    let redirect_uris = req.redirect_uris.clone().unwrap_or_default();
    let post_logout = req.post_logout_redirect_uris.clone().unwrap_or_default();
    let scopes = req
        .allowed_scopes
        .clone()
        .unwrap_or_else(|| vec!["openid".to_string()]);
    check_uris(&redirect_uris, "redirect_uris")?;
    check_uris(&post_logout, "post_logout_redirect_uris")?;
    check_scopes(&scopes)?;
    let confidential = req.confidential.unwrap_or(true);
    if !confidential
        && scopes
            .iter()
            .any(|s| flow::APP_SCOPES.contains(&s.as_str()))
    {
        return Err(ApiError::BadRequest(
            "an app without a secret can't act as itself, so it can't hold notify:members or training:members"
                .to_string(),
        ));
    }
    let secret = confidential.then(flow::new_secret);
    let secret_hash = secret
        .as_deref()
        .map(PasswordHashUtil::hash)
        .transpose()
        .map_err(|e| ApiError::InternalServerError(format!("hashing the secret: {e:?}")))?;
    let client = state.db.oauth_insert_client(&NewOauthClient {
        client_id: format!("css-{}", Uuid::new_v4().simple()),
        name: name.to_string(),
        secret_hash,
        confidential,
        redirect_uris,
        post_logout_redirect_uris: post_logout,
        allowed_scopes: scopes,
        skip_consent: req.skip_consent.unwrap_or(false),
        created_by: Some(user.0.id),
    })?;
    audit(
        &state,
        AuditEventType::OauthClientCreated,
        Some(user.0.id),
        json!({ "id": client.id, "client_id": client.client_id, "app": client.name,
                "scopes": client.allowed_scopes, "skip_consent": client.skip_consent }),
    );
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(CreatedClient {
            client,
            client_secret: secret,
        })),
    ))
}

async fn update_client(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ClientRequest>,
) -> Result<Json<ApiResponse<OauthClient>>, ApiError> {
    manager(&state, &user).await?;
    let Some(current) = state.db.oauth_get_client(id)? else {
        return Err(ApiError::NotFound("App not found".to_string()));
    };
    if req.confidential.is_some_and(|c| c != current.confidential) {
        return Err(ApiError::BadRequest(
            "whether an app has a secret can't change; register a new app".to_string(),
        ));
    }
    if let Some(u) = &req.redirect_uris {
        check_uris(u, "redirect_uris")?;
    }
    if let Some(u) = &req.post_logout_redirect_uris {
        check_uris(u, "post_logout_redirect_uris")?;
    }
    if let Some(s) = &req.allowed_scopes {
        check_scopes(s)?;
        if !current.confidential && s.iter().any(|x| flow::APP_SCOPES.contains(&x.as_str())) {
            return Err(ApiError::BadRequest(
                "an app without a secret can't act as itself, so it can't hold notify:members or training:members"
                    .to_string(),
            ));
        }
    }
    if req.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ApiError::BadRequest("name can't be empty".to_string()));
    }
    let Some(updated) = state.db.oauth_update_client(
        id,
        store::OauthClientChanges {
            name: req.name.map(|n| n.trim().to_string()),
            redirect_uris: req.redirect_uris,
            post_logout_redirect_uris: req.post_logout_redirect_uris,
            allowed_scopes: req.allowed_scopes,
            skip_consent: req.skip_consent,
            disabled: req.disabled,
            updated_at: None,
        },
    )?
    else {
        return Err(ApiError::NotFound("App not found".to_string()));
    };
    audit(
        &state,
        AuditEventType::OauthClientUpdated,
        Some(user.0.id),
        json!({ "id": updated.id, "client_id": updated.client_id, "app": updated.name,
                "scopes": updated.allowed_scopes, "skip_consent": updated.skip_consent,
                "disabled": updated.disabled }),
    );
    Ok(Json(ApiResponse::success(updated)))
}

async fn delete_client(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Value>>, ApiError> {
    manager(&state, &user).await?;
    let Some(c) = state.db.oauth_get_client(id)? else {
        return Err(ApiError::NotFound("App not found".to_string()));
    };
    state.db.oauth_delete_client(id)?;
    audit(
        &state,
        AuditEventType::OauthClientDeleted,
        Some(user.0.id),
        json!({ "id": c.id, "client_id": c.client_id, "app": c.name }),
    );
    Ok(Json(ApiResponse::success(json!({ "deleted": true }))))
}

async fn rotate_secret(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<CreatedClient>>, ApiError> {
    manager(&state, &user).await?;
    let secret = flow::new_secret();
    let hash = PasswordHashUtil::hash(&secret)
        .map_err(|e| ApiError::InternalServerError(format!("hashing the secret: {e:?}")))?;
    let Some(client) = state.db.oauth_set_client_secret(id, &hash)? else {
        return Err(ApiError::NotFound(
            "No app with a secret by that id".to_string(),
        ));
    };
    audit(
        &state,
        AuditEventType::OauthClientSecretRotated,
        Some(user.0.id),
        json!({ "id": client.id, "client_id": client.client_id, "app": client.name }),
    );
    Ok(Json(ApiResponse::success(CreatedClient {
        client,
        client_secret: Some(secret),
    })))
}

async fn rotate_keys(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<ApiResponse<Value>>, ApiError> {
    manager(&state, &user).await?;
    let kid = oidc::rotate_signing_key(&state, Some(user.0.id))?;
    Ok(Json(ApiResponse::success(json!({ "kid": kid }))))
}
