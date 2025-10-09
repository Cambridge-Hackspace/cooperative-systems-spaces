use std::fmt::Display;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use argon2::{
    password_hash::{PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
    PasswordHash as ArgonPasswordHash,
};

use crate::{database::DatabaseManager, models::{User, UserRole}, AppState};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // Subject (user ID)
    pub exp: usize,  // Expiration time
    pub iat: usize,  // Issued at
    pub user_id: Uuid,
    pub username: String,
    pub role: UserRole,
}

impl Claims {
    pub fn new(user: &User, jwt_secret: &str) -> Result<String, AuthError> {
        let expiration = Utc::now()
            .checked_add_signed(Duration::hours(24))
            .expect("valid timestamp")
            .timestamp();

        let claims = Claims {
            sub: user.id.to_string(),
            exp: expiration as usize,
            iat: Utc::now().timestamp() as usize,
            user_id: user.id,
            username: user.username.clone(),
            role: user.role.clone(),
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
            AuthError::UserNotFound => write!(f, "User not found"),
            AuthError::UserInactive => write!(f, "User account is inactive"),
            AuthError::InvalidPassword(msg) => write!(f, "Invalid password: {}", msg),
            AuthError::InternalError => write!(f, "Internal server error"),
        }
    }
}

impl std::error::Error for AuthError {}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AuthError::WrongCredentials => (StatusCode::UNAUTHORIZED, "Wrong credentials"),
            AuthError::MissingCredentials => (StatusCode::BAD_REQUEST, "Missing credentials"),
            AuthError::TokenCreation => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create token"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token"),
            AuthError::UserNotFound => (StatusCode::NOT_FOUND, "User not found"),
            AuthError::UserInactive => (StatusCode::FORBIDDEN, "User account is inactive"),
            AuthError::InvalidPassword(_) => (StatusCode::BAD_REQUEST, "Invalid password"),
            AuthError::InternalError => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        let body = Json(serde_json::json!({
            "error": error_message,
        }));

        (status, body).into_response()
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

        let password_hash = argon2.hash_password(password.as_bytes(), &salt)
            .map_err(|e| AuthError::InvalidPassword(e.to_string()))?;

        Ok(password_hash.to_string())
    }

    /// Verify a password against a hash
    pub fn verify(password: &str, hash: &str) -> Result<bool, AuthError> {
        let parsed_hash = ArgonPasswordHash::new(hash)
            .map_err(|e| AuthError::InvalidPassword(e.to_string()))?;

        let argon2 = Argon2::default();
        Ok(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok())
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

    pub fn authenticate_user(&self, username_or_email: &str, password: &str) -> Result<User, AuthError> {
        // Try to find user by username first, then by email
        let user = self.db.find_user_by_username(username_or_email)
            .map_err(|_| AuthError::InternalError)?
            .or_else(|| {
                self.db.find_user_by_email(username_or_email)
                    .map_err(|_| AuthError::InternalError)
                    .unwrap_or(None)
            })
            .ok_or(AuthError::WrongCredentials)?;

        if !user.is_active {
            return Err(AuthError::UserInactive);
        }

        if !PasswordHashUtil::verify(password, &user.password_hash)? {
            return Err(AuthError::WrongCredentials);
        }

        Ok(user)
    }

    pub fn create_token(&self, user: &User) -> Result<String, AuthError> {
        Claims::new(user, self.jwt_secret)
    }

    pub fn verify_token(&self, token: &str) -> Result<Claims, AuthError> {
        Claims::verify_token(token, self.jwt_secret)
    }

    pub fn get_user_from_token(&self, token: &str) -> Result<User, AuthError> {
        let claims = self.verify_token(token)?;
        let user = self.db.find_user_by_id(claims.user_id)
            .map_err(|_| AuthError::InternalError)?
            .ok_or(AuthError::UserNotFound)?;

        if !user.is_active {
            return Err(AuthError::UserInactive);
        }

        Ok(user)
    }
}

// JWT middleware for extracting user from request
#[derive(Clone)]
pub struct AuthUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state: AppState = AppState::from_ref(state);

        // Extract the token from Authorization header
        let auth_header = parts.headers.get("authorization")
            .ok_or(AuthError::MissingCredentials)?
            .to_str()
            .map_err(|_| AuthError::InvalidToken)?;

        let token = auth_header.strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?;

        // Create auth service
        let config = app_state.config_manager.get_config();
        let auth_service = AuthService::new(
            &app_state.db,
            &config.auth.jwt_secret,
        );

        // Verify token and get user
        let user = auth_service.get_user_from_token(token)?;

        Ok(AuthUser(user))
    }
}

// Optional: Admin-only middleware
#[derive(Clone)]
pub struct AdminUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        
        if !user.role.can_access_admin() {
            return Err(AuthError::InvalidToken); // Could create a specific "Forbidden" error
        }

        Ok(AdminUser(user))
    }
}

// Staff-level access middleware
#[derive(Clone)]
pub struct StaffUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for StaffUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;

        if !user.role.can_access_staff() {
            return Err(AuthError::InvalidToken); // Could create a specific "Forbidden" error
        }

        Ok(StaffUser(user))
    }
}

// Member-level access middleware (excludes unknown and newbie)
#[derive(Clone)]
pub struct MemberUser(pub User);

#[async_trait]
impl<S> FromRequestParts<S> for MemberUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;

        if !user.role.can_access_member() {
            return Err(AuthError::InvalidToken); // Could create a specific "Forbidden" error
        }

        Ok(MemberUser(user))
    }
}