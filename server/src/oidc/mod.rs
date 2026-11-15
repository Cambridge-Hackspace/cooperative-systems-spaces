//! CS:S as an OpenID Connect provider (#225).
//!
//! Other programs (POP, the print tower, first) sign members in through CS:S,
//! the way they would through Google or GitHub, and hold app tokens of their
//! own through the client-credentials grant. The protocol endpoints live in
//! `api::oidc`; this module holds what they share: the issuer, the signing key
//! and its rotation, and making and checking tokens.

pub mod flow;
pub mod keys;
pub mod relay;
pub mod store;

use std::sync::{Arc, Mutex, OnceLock};

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::models::{AuditEventType, NewAuditLog, NewOauthSigningKey, User};
use crate::AppState;

/// What an access token is for: CS:S's own API, under the issuer.
pub fn api_audience(state: &AppState) -> String {
    format!("{}/api", issuer(state))
}

pub fn enabled(state: &AppState) -> bool {
    state.config_manager.get_config().oidc.enabled
}

/// The issuer every token names and discovery is served under.
pub fn issuer(state: &AppState) -> String {
    let config = state.config_manager.get_config();
    config.oidc.issuer_or(&config.site.site_url)
}

fn sealing_key(state: &AppState) -> Result<[u8; 32], ApiError> {
    let config = state.config_manager.get_config();
    let configured = config.oidc.key_encryption_key.trim();
    keys::sealing_key(
        (!configured.is_empty()).then_some(configured),
        &config.auth.jwt_secret,
    )
    .map_err(ApiError::InternalServerError)
}

/// The signing key, unsealed once and kept: unsealing on every token would be
/// a needless RSA key parse per request.
fn key_cache() -> &'static Mutex<Option<Arc<keys::SigningKey>>> {
    static CACHE: OnceLock<Mutex<Option<Arc<keys::SigningKey>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Make a new signing key and retire the current one. The retired key stays
/// published until every token it signed has expired.
pub fn rotate_signing_key(state: &AppState, actor: Option<Uuid>) -> Result<String, ApiError> {
    let key = keys::SigningKey::generate()
        .map_err(|e| ApiError::InternalServerError(format!("signing key: {e:?}")))?;
    let sealed = key
        .seal(&sealing_key(state)?)
        .map_err(|e| ApiError::InternalServerError(format!("sealing key: {e:?}")))?;
    state.db.oauth_rotate_signing_key(&NewOauthSigningKey {
        kid: key.kid.clone(),
        private_key_sealed: sealed,
        public_jwk: key.public_jwk(),
    })?;
    let kid = key.kid.clone();
    *key_cache().lock().expect("key cache") = Some(Arc::new(key));
    let _ = state.db.create_audit_log(&NewAuditLog {
        event_type: AuditEventType::OauthSigningKeyRotated.as_str().to_string(),
        user_id: actor,
        actor_id: actor,
        event_data: json!({ "kid": kid }),
        ip_address: None,
        user_agent: None,
    });
    Ok(kid)
}

/// The key that signs now. Made if there is none, if the newest is due for
/// rotation, or if it can't be opened (the sealing key changed).
pub fn current_key(state: &AppState) -> Result<Arc<keys::SigningKey>, ApiError> {
    let config = state.config_manager.get_config();
    let newest = state
        .db
        .oauth_signing_keys()?
        .into_iter()
        .find(|k| k.retired_at.is_none());
    let Some(row) = newest else {
        rotate_signing_key(state, None)?;
        return current_key_cached(state);
    };
    if row.created_at < Utc::now() - Duration::days(config.oidc.key_rotation_days) {
        rotate_signing_key(state, None)?;
        return current_key_cached(state);
    }
    if let Some(k) = key_cache().lock().expect("key cache").as_ref() {
        if k.kid == row.kid {
            return Ok(k.clone());
        }
    }
    match keys::SigningKey::unseal(&row.private_key_sealed, &sealing_key(state)?) {
        Ok(k) => {
            let k = Arc::new(k);
            *key_cache().lock().expect("key cache") = Some(k.clone());
            Ok(k)
        }
        Err(_) => {
            tracing::warn!(
                "OIDC signing key {} cannot be opened (the sealing key or auth.jwt_secret \
                 changed); making a new one. Tokens signed with the old key no longer verify.",
                row.kid
            );
            rotate_signing_key(state, None)?;
            current_key_cached(state)
        }
    }
}

fn current_key_cached(state: &AppState) -> Result<Arc<keys::SigningKey>, ApiError> {
    if let Some(k) = key_cache().lock().expect("key cache").as_ref() {
        return Ok(k.clone());
    }
    current_key(state)
}

/// The public keys a relying party may verify against right now.
pub fn published_jwks(state: &AppState) -> Result<Vec<Value>, ApiError> {
    // Signing makes a key if there isn't one, so the first JWKS request after
    // enabling the provider already has something to publish.
    current_key(state)?;
    let config = state.config_manager.get_config();
    let rows = state.db.oauth_signing_keys()?;
    let ages: Vec<(String, Option<i64>)> = rows
        .iter()
        .map(|k| (k.kid.clone(), k.retired_at.map(|t| t.timestamp())))
        .collect();
    let keep = keys::published_kids(
        &ages,
        Utc::now().timestamp(),
        config.oidc.longest_token_secs(),
    );
    Ok(rows
        .into_iter()
        .filter(|k| keep.contains(&k.kid))
        .map(|k| k.public_jwk)
        .collect())
}

pub fn sign(state: &AppState, claims: &Value) -> Result<String, ApiError> {
    current_key(state)?
        .sign(claims)
        .map_err(|e| ApiError::InternalServerError(format!("signing: {e:?}")))
}

/// Why a token was refused. Each reads as a 401 to the caller; the reason is
/// for the log.
#[derive(Debug, PartialEq, Eq)]
pub enum TokenRefused {
    Signature(keys::KeyError),
    Issuer,
    Audience,
    Expired,
}

/// Check a token we issued: signature against a published key, issuer, expiry,
/// and, when given, audience. `allow_expired` is for an `id_token_hint`, which
/// may be presented after the ID token itself has expired.
pub fn verify(
    state: &AppState,
    token: &str,
    audience: Option<&str>,
    allow_expired: bool,
) -> Result<Value, TokenRefused> {
    let published =
        published_jwks(state).map_err(|_| TokenRefused::Signature(keys::KeyError::Crypto))?;
    let claims = keys::verify(token, &published).map_err(TokenRefused::Signature)?;
    if claims.get("iss").and_then(Value::as_str) != Some(issuer(state).as_str()) {
        return Err(TokenRefused::Issuer);
    }
    if let Some(aud) = audience {
        if claims.get("aud").and_then(Value::as_str) != Some(aud) {
            return Err(TokenRefused::Audience);
        }
    }
    let exp = claims.get("exp").and_then(Value::as_i64).unwrap_or(0);
    if !allow_expired && exp <= Utc::now().timestamp() {
        return Err(TokenRefused::Expired);
    }
    Ok(claims)
}

/// The claims a member shares with an app, by scope. `sub` is the user's UUID,
/// never the email: members change addresses and accounts are merged (#118).
pub fn user_claims(state: &AppState, user: &User, scopes: &[String]) -> Result<Value, ApiError> {
    let mut claims = json!({ "sub": user.id.to_string() });
    let has = |s: &str| scopes.iter().any(|x| x == s);
    if has("profile") {
        claims["name"] = json!(user.full_name);
        claims["preferred_username"] = json!(user.username);
    }
    if has("email") {
        claims["email"] = json!(user.email);
        claims["email_verified"] = json!(user.email_verified_at.is_some());
    }
    if has("roles") {
        let (roles, _) = state.db.user_roles_and_permissions(user.id)?;
        claims["roles"] = json!(roles);
    }
    Ok(claims)
}

/// An access token for a member, or (with `user` None) for the app itself.
pub fn access_token(
    state: &AppState,
    client_id: &str,
    user: Option<Uuid>,
    scopes: &[String],
) -> Result<(String, i64), ApiError> {
    let config = state.config_manager.get_config();
    let now = Utc::now().timestamp();
    let ttl = config.oidc.access_token_ttl_secs;
    let sub = user.map_or_else(|| format!("client:{client_id}"), |u| u.to_string());
    let token = sign(
        state,
        &json!({
            "iss": issuer(state),
            "aud": api_audience(state),
            "sub": sub,
            "client_id": client_id,
            "scope": scopes.join(" "),
            "iat": now,
            "exp": now + ttl,
            "jti": Uuid::new_v4().to_string(),
        }),
    )?;
    Ok((token, ttl))
}
