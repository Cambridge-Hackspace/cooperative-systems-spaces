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
            // Delegated, not re-implemented.
            //
            // These two conversions used to classify the same diesel error
            // differently: this one recognised only NotFound and turned
            // everything else into a 500, while `From<diesel::result::Error>`
            // below mapped a unique violation to 409. Which of the two a
            // failure took depended on whether the calling code had wrapped it
            // in DatabaseError first, so one concurrent profile-config edit
            // answered 409 and another answered 500 for the identical
            // constraint violation, and no reading of either function alone
            // showed anything wrong.
            //
            // Delegating is what makes them agree permanently. A third copy of
            // the mapping — even a correct one — would be free to drift again.
            DatabaseError::Diesel(diesel_err) => ApiError::from(diesel_err),
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
//
// The single place a database failure becomes an HTTP status. Every arm below
// answers one question: can the caller do anything about this? A constraint the
// caller's own input violated is theirs to fix and gets a 4xx; anything else is
// ours and gets a 500.
//
// Only the kinds diesel classifies structurally are matched. Postgres reports a
// great deal more through SQLSTATE — 22P05 untranslatable_character among
// them — but `DatabaseErrorInformation` exposes message, details, hint, table,
// column, constraint and statement position, and no SQLSTATE. Recovering one by
// matching on the message text would key this function on English prose that
// changes with the server's lc_messages, which is a worse failure than the one
// it fixes: it would work in testing and stop working in a deployment whose
// locale differs, silently, in the direction of calling a 4xx a 500.
//
// So untranslatable text still becomes a 500. That is a real finding, it is
// recorded in TESTING.md rather than papered over here, and the stack battery
// reports it on every hostile-encoding run.
impl From<diesel::result::Error> for ApiError {
    fn from(diesel_error: diesel::result::Error) -> Self {
        use diesel::result::DatabaseErrorKind as Kind;
        use diesel::result::Error as E;

        match diesel_error {
            E::NotFound => ApiError::NotFound("Requested resource not found".to_string()),

            // The caller asked for something that already exists.
            E::DatabaseError(Kind::UniqueViolation, _) => {
                ApiError::Conflict("Resource already exists".to_string())
            }

            // The caller referenced a row that is not there, or tried to remove
            // one something else still points at. Either way the request is
            // wrong rather than the server, and 409 says so without leaking
            // which direction the reference ran.
            E::DatabaseError(Kind::ForeignKeyViolation, _) => ApiError::Conflict(
                "Referenced resource does not exist or is still in use".to_string(),
            ),

            // A field the schema requires was absent or null.
            E::DatabaseError(Kind::NotNullViolation, _) => {
                ApiError::BadRequest("A required field was missing".to_string())
            }

            // A value the schema constrains was out of range. The door and tool
            // enums are text columns with CHECK constraints, so this is the arm
            // an unrecognised kind or effect lands in.
            E::DatabaseError(Kind::CheckViolation, _) => {
                ApiError::BadRequest("A value was not one this field accepts".to_string())
            }

            // Postgres could not serialise concurrent transactions. Retrying
            // genuinely may work, which is exactly what 409 tells a caller and
            // 500 does not.
            E::DatabaseError(Kind::SerializationFailure, _) => ApiError::Conflict(
                "The request conflicted with a concurrent change; retry".to_string(),
            ),

            // Ours, all of them: a read-only transaction, a closed connection,
            // a command that could not be sent, a query the code built wrong.
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use diesel::result::DatabaseErrorKind as Kind;
    use diesel::result::Error as DieselError;

    fn db_error(kind: Kind, message: &str) -> DieselError {
        DieselError::DatabaseError(kind, Box::new(message.to_string()))
    }

    fn status_of(err: ApiError) -> StatusCode {
        err.into_response().status()
    }

    async fn body_of(err: ApiError) -> serde_json::Value {
        let bytes = to_bytes(err.into_response().into_body(), 64 * 1024)
            .await
            .expect("the error body is small and always present");
        serde_json::from_slice(&bytes).expect("every error response is JSON")
    }

    /// The test that would have caught the defect this module was changed for.
    ///
    /// The same `diesel::result::Error` reached `IntoResponse` by two routes —
    /// directly, and wrapped in `DatabaseError::Diesel` — and the two disagreed
    /// about every kind except `NotFound`. Which one a failure took depended on
    /// whether the calling code had wrapped it first, so one concurrent
    /// profile-config edit answered 409 and another answered 500 for the
    /// identical unique violation.
    ///
    /// Asserting the two paths agree is stronger than asserting either one's
    /// table, because it keeps holding when the table changes. Both were
    /// self-consistent; neither was wrong on its own terms; the defect only
    /// existed in the comparison.
    #[test]
    fn the_two_conversion_paths_agree_on_every_kind() {
        let kinds = [
            Kind::UniqueViolation,
            Kind::ForeignKeyViolation,
            Kind::NotNullViolation,
            Kind::CheckViolation,
            Kind::SerializationFailure,
            Kind::ReadOnlyTransaction,
            Kind::ClosedConnection,
            Kind::UnableToSendCommand,
            Kind::Unknown,
        ];

        for kind in kinds {
            let direct = status_of(ApiError::from(db_error(kind, "boom")));
            let wrapped = status_of(ApiError::from(DatabaseError::Diesel(db_error(
                kind, "boom",
            ))));
            assert_eq!(
                direct, wrapped,
                "{kind:?}: the direct path answers {direct} and the DatabaseError \
                 path answers {wrapped}. Callers cannot know which one they are \
                 on, so a difference here is a status code decided by an \
                 implementation detail."
            );
        }

        // NotFound is not a DatabaseError(kind, _), so it is checked separately
        // rather than left out — it is the one case the old code got right, and
        // a regression that "fixed" the others by breaking this would otherwise
        // pass.
        assert_eq!(
            status_of(ApiError::from(DieselError::NotFound)),
            status_of(ApiError::from(DatabaseError::Diesel(DieselError::NotFound))),
        );
    }

    #[test]
    fn a_constraint_the_caller_violated_is_a_4xx() {
        // The whole question each arm answers: can the caller do anything about
        // this? If yes it is theirs and gets a 4xx; if no it is ours.
        assert_eq!(
            status_of(ApiError::from(db_error(
                Kind::UniqueViolation,
                "duplicate key"
            ))),
            StatusCode::CONFLICT
        );
        assert_eq!(
            status_of(ApiError::from(db_error(Kind::ForeignKeyViolation, "fk"))),
            StatusCode::CONFLICT
        );
        assert_eq!(
            status_of(ApiError::from(db_error(
                Kind::SerializationFailure,
                "40001"
            ))),
            StatusCode::CONFLICT
        );
        assert_eq!(
            status_of(ApiError::from(db_error(
                Kind::NotNullViolation,
                "null value"
            ))),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_of(ApiError::from(db_error(Kind::CheckViolation, "check"))),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn everything_else_is_ours_and_stays_a_500() {
        for kind in [
            Kind::ReadOnlyTransaction,
            Kind::ClosedConnection,
            Kind::Unknown,
        ] {
            assert_eq!(
                status_of(ApiError::from(db_error(kind, "x"))),
                StatusCode::INTERNAL_SERVER_ERROR,
                "{kind:?}"
            );
        }
        // Named explicitly, because it is the arm the hostile-encoding finding
        // lands in and it must not drift into a 4xx by accident. Postgres
        // reports untranslatable text as SQLSTATE 22P05, diesel has no
        // structured kind for it, and `DatabaseErrorInformation` exposes no
        // SQLSTATE — so classifying it would mean matching English prose that
        // changes with the server's lc_messages. It stays a 500 and is recorded
        // in TESTING.md rather than guessed at here.
        assert_eq!(
            status_of(ApiError::from(db_error(
                Kind::Unknown,
                "character with byte sequence 0xf0 0x9f 0x9a 0xa7 in encoding \
                 \"UTF8\" has no equivalent in encoding \"LATIN1\""
            ))),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn a_database_error_never_leaks_the_database_message() {
        // The diesel message names tables, columns and constraints. It belongs
        // in the log, not in a response to an unauthenticated caller.
        let err = ApiError::from(db_error(
            Kind::Unknown,
            "relation \"user_mfa_recovery_codes\" does not exist",
        ));
        let body = body_of(err).await;
        assert_eq!(body["success"], serde_json::json!(false));
        let message = body["error"].as_str().expect("error is a string");
        assert!(
            !message.contains("user_mfa_recovery_codes"),
            "the response repeated the database's own message: {message}"
        );
        assert_eq!(message, "Database operation failed");
    }

    #[tokio::test]
    async fn a_conflict_does_say_what_happened() {
        // The scrub above must not turn into "say nothing about anything". A
        // 409 whose body is empty is a 409 the frontend cannot render, and the
        // message here contains no schema detail to leak.
        let body = body_of(ApiError::from(db_error(Kind::UniqueViolation, "dup"))).await;
        assert_eq!(body["success"], serde_json::json!(false));
        assert_eq!(body["error"], serde_json::json!("Resource already exists"));
    }

    #[test]
    fn the_status_table_is_exactly_this() {
        // Every variant is listed. The enum has no wildcard arm in
        // IntoResponse, so adding a variant without deciding its status fails
        // to compile -- and adding one without listing it here leaves it
        // unasserted, which is what this comment exists to prevent.
        for (err, want) in [
            (
                ApiError::Unauthorized(String::new()),
                StatusCode::UNAUTHORIZED,
            ),
            (ApiError::Forbidden(String::new()), StatusCode::FORBIDDEN),
            (
                ApiError::ValidationError(String::new()),
                StatusCode::BAD_REQUEST,
            ),
            (ApiError::BadRequest(String::new()), StatusCode::BAD_REQUEST),
            (ApiError::NotFound(String::new()), StatusCode::NOT_FOUND),
            (ApiError::Conflict(String::new()), StatusCode::CONFLICT),
            (
                ApiError::TooManyRequests(String::new()),
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                ApiError::InternalServerError(String::new()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ApiError::DatabaseError(String::new()),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            (
                ApiError::NotImplemented(String::new()),
                StatusCode::NOT_IMPLEMENTED,
            ),
        ] {
            let described = err.to_string();
            assert_eq!(status_of(err), want, "{described}");
        }
    }

    #[test]
    fn a_pool_failure_is_never_mistaken_for_a_missing_row() {
        // The contract tier's offline fixture relies on this: 500 is the
        // universal reached-the-dead-pool signal, distinct in status from every
        // legitimate rejection. If a pool timeout ever became a 404, every
        // negative result in that tier would be silently reinterpreted.
        assert_eq!(
            status_of(ApiError::from(DatabaseError::ConnectionTimeout)),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_of(ApiError::from(DatabaseError::Other("anything".into()))),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
