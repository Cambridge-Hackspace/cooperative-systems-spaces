use axum::http::StatusCode;
use axum::{response::IntoResponse, Json};
use serde_json::json;
use std::fmt::Display;

use crate::auth::AuthError;
use crate::database::DatabaseError;

#[derive(Debug)]
pub enum ApiError {
    // Authentication errors
    Unauthorized(String),
    Forbidden(String),

    // Validation errors
    ValidationError(String),
    BadRequest(String),

    // Resource errors
    NotFound(String),
    Conflict(String),

    // Rate limiting errors
    TooManyRequests(String),

    // Server errors
    InternalServerError(String),
    DatabaseError(String),
    NotImplemented(String),
}

impl Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            ApiError::Forbidden(msg) => write!(f, "Forbidden: {}", msg),
            ApiError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            ApiError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            ApiError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ApiError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            ApiError::TooManyRequests(msg) => write!(f, "Too many requests: {}", msg),
            ApiError::InternalServerError(msg) => write!(f, "Internal server error: {}", msg),
            ApiError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
            ApiError::NotImplemented(msg) => write!(f, "Not implemented: {}", msg),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match &self {
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            ApiError::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            ApiError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg.clone()),
            ApiError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ApiError::DatabaseError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database operation failed".to_string(),
            ),
            ApiError::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg.clone()),
        };

        let body = Json(json!({
            "success": false,
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

// Convert AuthError to ApiError
impl From<AuthError> for ApiError {
    fn from(auth_error: AuthError) -> Self {
        match auth_error {
            AuthError::WrongCredentials => {
                ApiError::Unauthorized("Invalid credentials".to_string())
            }
            AuthError::MissingCredentials => {
                ApiError::Unauthorized("Missing authentication credentials".to_string())
            }
            AuthError::TokenCreation => {
                ApiError::InternalServerError("Failed to create authentication token".to_string())
            }
            AuthError::InvalidToken => {
                ApiError::Unauthorized("Invalid authentication token".to_string())
            }
            AuthError::UserNotFound => ApiError::NotFound("User not found".to_string()),
            AuthError::UserInactive => ApiError::Forbidden("User account is inactive".to_string()),
            AuthError::InvalidPassword(_) => {
                ApiError::BadRequest("Invalid password format".to_string())
            }
            AuthError::InternalError => {
                ApiError::InternalServerError("Authentication service error".to_string())
            }
        }
    }
}

// Convert DatabaseError to ApiError
impl From<DatabaseError> for ApiError {
    fn from(err: DatabaseError) -> Self {
        match err {
            DatabaseError::Pool(_) => {
                ApiError::InternalServerError("Database connection error".to_string())
            }
            DatabaseError::Diesel(diesel_err) => match diesel_err {
                diesel::result::Error::NotFound => {
                    ApiError::NotFound("Resource not found".to_string())
                }
                _ => ApiError::InternalServerError("Database error".to_string()),
            },
            DatabaseError::Migration(_) => {
                ApiError::InternalServerError("Database migration error".to_string())
            }
            DatabaseError::ConnectionTimeout => {
                ApiError::InternalServerError("Database connection timeout".to_string())
            }
            DatabaseError::Other(msg) => {
                ApiError::InternalServerError(format!("Database error: {}", msg))
            }
        }
    }
}

// Convert Diesel errors to ApiError
impl From<diesel::result::Error> for ApiError {
    fn from(diesel_error: diesel::result::Error) -> Self {
        match diesel_error {
            diesel::result::Error::NotFound => {
                ApiError::NotFound("Requested resource not found".to_string())
            }
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                _,
            ) => ApiError::Conflict("Resource already exists".to_string()),
            _ => ApiError::DatabaseError(format!("Database operation failed: {}", diesel_error)),
        }
    }
}

// Validation error helper
pub fn validation_error(message: &str) -> ApiError {
    ApiError::ValidationError(message.to_string())
}

// Convert r2d2 Pool errors to ApiError
impl From<diesel::r2d2::PoolError> for ApiError {
    fn from(err: diesel::r2d2::PoolError) -> Self {
        ApiError::InternalServerError(format!("Database connection pool error: {}", err))
    }
}
