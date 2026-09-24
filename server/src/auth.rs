use argon2::{
    password_hash::{PasswordHasher, PasswordVerifier, SaltString},
    Argon2, PasswordHash as ArgonPasswordHash,
};
use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use uuid::Uuid;

use crate::{database::DatabaseManager, models::User, AppState};

/// A fixed, valid Argon2 hash used to equalize the timing of a login for an
/// unknown user with one for a real user (#120/M11). Built once from the same
/// hasher, so a not-found verify costs the same KDF work as a real one; the
/// password is never expected to match it. A malformed constant would make
/// `verify` return early with a parse error and skip the KDF, which is exactly
/// the oracle this removes -- a unit test asserts it does real work.
static DUMMY_PASSWORD_HASH: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    PasswordHashUtil::hash("timing-equalization-dummy-not-a-real-password")
        .expect("hashing a fixed constant cannot fail")
});

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // Subject (user ID)
    pub exp: usize,  // Expiration time
    pub iat: usize,  // Issued at
    pub user_id: Uuid,
    pub username: String,
    /// Session-revocation epoch (#120/M9), checked against users.token_version
    /// on every request. `serde(default)` so a token minted before this field
    /// existed decodes as 0 and still matches an unbumped account -- no forced
    /// logout on deploy; only a credential change (which bumps the row) revokes.
    #[serde(default)]
    pub token_version: i32,
}

impl Claims {
    pub fn new(user: &User, jwt_secret: &str, expiration_hours: u32) -> Result<String, AuthError> {
        // #120/#10 (M6): the lifetime was hardcoded to 24h while the login
        // response reported auth.jwt_expiration_hours, so an operator who set 1h
        // got a token that actually lived 24h. The caller passes the configured
        // value, read at token-creation time so a config reload is not stale.
        let expiration = Utc::now()
            .checked_add_signed(Duration::hours(expiration_hours as i64))
            .expect("valid timestamp")
            .timestamp();

        let claims = Claims {
            sub: user.id.to_string(),
            exp: expiration as usize,
            iat: Utc::now().timestamp() as usize,
            user_id: user.id,
            username: user.username.clone(),
            token_version: user.token_version,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret.as_ref()),
        )
        .map_err(|_| AuthError::TokenCreation)?;

        Ok(token)
    }

    pub fn verify_token(token: &str, jwt_secret: &str) -> Result<Claims, AuthError> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(jwt_secret.as_ref()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| AuthError::InvalidToken)?;

        Ok(token_data.claims)
    }
}

#[derive(Debug)]
pub enum AuthError {
    WrongCredentials,
    MissingCredentials,
    TokenCreation,
    InvalidToken,
    /// The caller authenticated successfully and is not allowed to do this.
    ///
    /// Distinct from every other variant here, and the distinction is not
    /// pedantry. The three role gates below used to return `InvalidToken` for
    /// an insufficient role -- with a comment saying a Forbidden variant could
    /// be created -- so a Newbie touching an admin route was told 401. The
    /// frontend's axios interceptor calls `authStore.logout()` on **any** 401
    /// (utils/api.ts:83), which is the correct thing to do when a token has
    /// expired and exactly the wrong thing here: a member who reached an
    /// admin-only endpoint was silently signed out, with no message, and the
    /// obvious next step -- signing back in -- changed nothing.
    ///
    /// The carried string is the role requirement, not the user's role, because
    /// this is rendered to the caller.
    Forbidden(&'static str),
    UserNotFound,
    UserInactive,
    InvalidPassword(String),
    InternalError,
}

impl Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::WrongCredentials => write!(f, "Wrong credentials"),
            AuthError::MissingCredentials => write!(f, "Missing credentials"),
            AuthError::TokenCreation => write!(f, "Failed to create token"),
            AuthError::InvalidToken => write!(f, "Invalid token"),
            AuthError::Forbidden(what) => write!(f, "Forbidden: {} required", what),
            AuthError::UserNotFound => write!(f, "User not found"),
            AuthError::UserInactive => write!(f, "User account is inactive"),
            AuthError::InvalidPassword(msg) => write!(f, "Invalid password: {}", msg),
            AuthError::InternalError => write!(f, "Internal server error"),
        }
    }
}

impl std::error::Error for AuthError {}

impl IntoResponse for AuthError {
    /// Delegated to `ApiError`, which is the one place a status and an error
    /// envelope are decided.
    ///
    /// This used to be a second, complete copy of the mapping, and it built a
    /// *different body*: `{"error": "..."}` where every handler-originated
    /// error is `{"success": false, "error": "..."}`. So a rejected request
    /// carried one shape when an extractor refused it and another when a
    /// handler did -- across the whole API, on the paths a client is most
    /// likely to be handling programmatically.
    ///
    /// The seeded fuzz tier found it, from a standing start, with an oracle
    /// that knows nothing about any endpoint: "anything answering with a JSON
    /// content type and an envelope-shaped body must fill in `success`". It
    /// reported it on twenty-three different routes in four hundred requests.
    ///
    /// Two implementations of one mapping is the same defect this codebase
    /// already had twice -- in the diesel conversions, and in the role gates.
    /// Delegation is the only version that cannot drift.
    fn into_response(self) -> Response {
        crate::api::errors::ApiError::from(self).into_response()
    }
}

// Password hashing utilities using Argon2
/// Utility struct for password hashing operations
pub struct PasswordHashUtil;

impl PasswordHashUtil {
    /// Hash a password using Argon2
    pub fn hash(password: &str) -> Result<String, AuthError> {
        use rand_core::OsRng;

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AuthError::InvalidPassword(e.to_string()))?;

        Ok(password_hash.to_string())
    }

    /// Verify a password against a hash
    pub fn verify(password: &str, hash: &str) -> Result<bool, AuthError> {
        let parsed_hash =
            ArgonPasswordHash::new(hash).map_err(|e| AuthError::InvalidPassword(e.to_string()))?;

        let argon2 = Argon2::default();
        Ok(argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }
}

// Authentication service
pub struct AuthService<'a> {
    pub db: &'a DatabaseManager,
    pub jwt_secret: &'a str,
}

impl<'a> AuthService<'a> {
    pub fn new(db: &'a DatabaseManager, jwt_secret: &'a str) -> Self {
        Self { db, jwt_secret }
    }

    pub fn authenticate_user(
        &self,
        username_or_email: &str,
        password: &str,
    ) -> Result<User, AuthError> {
        // Try to find user by username first, then by email
        let user = self
            .db
            .find_user_by_username(username_or_email)
            .map_err(|_| AuthError::InternalError)?
            .or_else(|| {
                self.db
                    .find_user_by_email(username_or_email)
                    .map_err(|_| AuthError::InternalError)
                    .unwrap_or(None)
            });

        // #120/M11: a login for a user who does not exist must cost the same as
        // one for a user who does, and must not reveal account status. Before,
        // an unknown user returned WrongCredentials with no Argon2 verify (fast),
        // while a real user paid the full ~hundreds-of-ms verify -- a username
        // timing oracle -- and an inactive account was rejected *before* the
        // password check, leaking its existence to an unauthenticated prober. So:
        // always run one verify (the real hash when found, a fixed dummy of the
        // same cost when not) and disclose is_active only after a correct password.
        match user {
            Some(user) => {
                if !PasswordHashUtil::verify(password, &user.password_hash)? {
                    return Err(AuthError::WrongCredentials);
                }
                if !user.is_active {
                    return Err(AuthError::UserInactive);
                }
                Ok(user)
            }
            None => {
                let _ = PasswordHashUtil::verify(password, &DUMMY_PASSWORD_HASH);
                Err(AuthError::WrongCredentials)
            }
        }
    }

    pub fn create_token(&self, user: &User, expiration_hours: u32) -> Result<String, AuthError> {
        Claims::new(user, self.jwt_secret, expiration_hours)
    }

    pub fn verify_token(&self, token: &str) -> Result<Claims, AuthError> {
        Claims::verify_token(token, self.jwt_secret)
    }

    pub fn get_user_from_token(&self, token: &str) -> Result<User, AuthError> {
        let claims = self.verify_token(token)?;
        let user = self
            .db
            .find_user_by_id(claims.user_id)
            .map_err(|_| AuthError::InternalError)?
            .ok_or(AuthError::UserNotFound)?;

        if !user.is_active {
            return Err(AuthError::UserInactive);
        }

        // #120/M9: reject a token minted before the user's current epoch. A
        // password change bumps users.token_version, so a session issued before
        // it is refused here even though the JWT signature is still valid.
        if claims.token_version != user.token_version {
            return Err(AuthError::InvalidToken);
        }

        Ok(user)
    }
}

// JWT middleware for extracting user from request
#[derive(Clone)]
pub struct AuthUser(pub User);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state: AppState = AppState::from_ref(state);

        // Extract the token from Authorization header
        let auth_header = parts
            .headers
            .get("authorization")
            .ok_or(AuthError::MissingCredentials)?
            .to_str()
            .map_err(|_| AuthError::InvalidToken)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?;

        // Create auth service
        let config = app_state.config_manager.get_config();
        let auth_service = AuthService::new(&app_state.db, &config.auth.jwt_secret);

        // Verify token and get user
        let user = auth_service.get_user_from_token(token)?;

        Ok(AuthUser(user))
    }
}

/// Extractor for a registered edge device, authenticated by its `auth_token`
/// (issued at device registration). Used by `/api/devices/ws` and the
/// inline-authenticated ToolGuard endpoints.
#[derive(Clone)]
pub struct DeviceAuth {
    pub device_id: uuid::Uuid,
    pub token: String,
}

impl<S> FromRequestParts<S> for DeviceAuth
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state: AppState = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get("authorization")
            .ok_or(AuthError::MissingCredentials)?
            .to_str()
            .map_err(|_| AuthError::InvalidToken)?;
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?;

        let (device_id, token_value) = app_state
            .db
            .find_device_by_auth_token(token)
            .map_err(|_| AuthError::InternalError)?
            .ok_or(AuthError::InvalidToken)?;

        Ok(DeviceAuth {
            device_id,
            token: token_value,
        })
    }
}

// Optional: Admin-only middleware
#[derive(Clone)]
pub struct AdminUser(pub User);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        let app_state: AppState = AppState::from_ref(state);

        if !app_state
            .db
            .user_has_permission(user.id, "admin.access")
            .map_err(|_| AuthError::InternalError)?
        {
            return Err(AuthError::Forbidden("administrator access"));
        }

        Ok(AdminUser(user))
    }
}

// Staff-level access middleware
#[derive(Clone)]
pub struct StaffUser(pub User);

impl<S> FromRequestParts<S> for StaffUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        let app_state: AppState = AppState::from_ref(state);

        if !app_state
            .db
            .user_has_permission(user.id, "staff.access")
            .map_err(|_| AuthError::InternalError)?
        {
            return Err(AuthError::Forbidden("staff access"));
        }

        Ok(StaffUser(user))
    }
}

// Member-level access middleware (excludes unknown and newbie)
#[derive(Clone)]
pub struct MemberUser(pub User);

impl<S> FromRequestParts<S> for MemberUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        let app_state: AppState = AppState::from_ref(state);

        if !app_state
            .db
            .user_has_permission(user.id, "member.access")
            .map_err(|_| AuthError::InternalError)?
        {
            return Err(AuthError::Forbidden("member access"));
        }

        Ok(MemberUser(user))
    }
}

/// Extractor for a launched cmi5 session, authenticated by the session
/// credential the content received from the `fetch` exchange.
///
/// This is deliberately *not* a JWT and shares nothing with the user-auth path:
/// the credential is an opaque token whose hash the [`crate::cmi5::Cmi5Service`]
/// resolves to the one registration it belongs to. It authorizes only the LRS
/// sub-routes, and only for its own registration/actor/activity — a learner
/// running the content cannot use it to reach any other API. Modelled on
/// [`DeviceAuth`]: a bearer token resolved against the database, not a role.
#[derive(Clone)]
pub struct Cmi5SessionAuth(pub crate::cmi5::Cmi5SessionContext);

impl<S> FromRequestParts<S> for Cmi5SessionAuth
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state: AppState = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get("authorization")
            .ok_or(AuthError::MissingCredentials)?
            .to_str()
            .map_err(|_| AuthError::InvalidToken)?;
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?;

        match app_state.cmi5_service.resolve_session(token) {
            Ok(Some(ctx)) => Ok(Cmi5SessionAuth(ctx)),
            // A resolvable-but-invalid credential (unknown or expired) is 401,
            // not 403: it is a bad credential, not an authenticated party
            // reaching past its scope. Scope violations are enforced per
            // statement in the LRS handlers, where they answer 403.
            Ok(None) => Err(AuthError::InvalidToken),
            Err(_) => Err(AuthError::InternalError),
        }
    }
}

#[cfg(test)]
mod token_ttl_tests {
    use super::*;

    fn sample_user() -> User {
        User {
            id: uuid::Uuid::nil(),
            username: "ttl".to_string(),
            email: "ttl@example.com".to_string(),
            password_hash: String::new(),
            full_name: "TTL User".to_string(),
            is_active: true,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .expect("epoch")
                .naive_utc(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .expect("epoch")
                .naive_utc(),
            profile: serde_json::Value::Null,
            meta: serde_json::Value::Null,
            mfa_enrolled_at: None,
            email_verified_at: None,
            mailing_list_opt_out_at: None,
            membership_next_due_at: None,
            stripe_customer_id: None,
            stripe_subscription_id: None,
            subscription_status: None,
            token_version: 0,
        }
    }

    #[test]
    fn token_lifetime_honours_the_configured_hours() {
        // #120/#10 (M6): the lifetime was hardcoded to 24h while the response
        // reported the configured value. A 1-hour token must now actually expire
        // in ~3600s. Mutation check: restore Duration::hours(24) and this fails
        // (86400 is nowhere near 3600). The +/-1 tolerance covers the two
        // separate Utc::now() calls in Claims::new straddling a second boundary.
        for hours in [1u32, 3, 24] {
            let token = Claims::new(&sample_user(), "test-secret", hours).expect("mints a token");
            let claims = Claims::verify_token(&token, "test-secret").expect("verifies");
            let ttl = claims.exp as i64 - claims.iat as i64;
            let want = hours as i64 * 3600;
            assert!(
                (ttl - want).abs() <= 1,
                "a {hours}h token lived {ttl}s, expected ~{want}s"
            );
        }
    }

    #[test]
    fn a_token_carries_and_round_trips_the_epoch() {
        // #120/M9: Claims must carry user.token_version so the extractor can
        // compare it against the row on every request. Mutation check: drop
        // token_version from Claims::new and this reads back 0 instead of 7.
        let mut user = sample_user();
        user.token_version = 7;
        let token = Claims::new(&user, "test-secret", 1).expect("mints a token");
        let claims = Claims::verify_token(&token, "test-secret").expect("verifies");
        assert_eq!(claims.token_version, 7);
    }

    #[test]
    fn the_dummy_hash_makes_the_not_found_path_do_real_work() {
        // #120/M11: the unknown-user path verifies against DUMMY_PASSWORD_HASH so
        // its timing matches a real user's. A malformed dummy would make verify
        // return Err early and skip the KDF -- reopening the timing oracle. This
        // proves the dummy parses and verify runs the comparison (Ok(false)).
        assert_eq!(
            PasswordHashUtil::verify("anything", &DUMMY_PASSWORD_HASH).ok(),
            Some(false)
        );
    }
}
