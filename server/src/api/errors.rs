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

impl ApiError {
    /// Does this map onto a 5xx?
    ///
    /// Kept beside the status table in `into_response` on purpose: if a variant
    /// is added there and not here, the two disagree about what counts as the
    /// server's fault, and `from_db` logs at the wrong level.
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            Self::InternalServerError(_) | Self::DatabaseError(_) | Self::NotImplemented(_)
        )
    }

    /// Classify a database error, and log it at the level its status deserves.
    ///
    /// This exists because the handlers did two things wrong at once, and
    /// fixing either alone does not work.
    ///
    /// They answered 500 for everything -- `map_err(|e| { tracing::error!(..);
    /// ApiError::InternalServerError("Failed to X") })` -- so a caller naming a
    /// row that does not exist was told the server had broken. `From` already
    /// classified these correctly and the handler was discarding it.
    ///
    /// And they logged at ERROR unconditionally. The stack battery's `logs`
    /// stage treats server ERROR output as an oracle, so correcting only the
    /// status would leave a correct 404 accompanied by an ERROR line and the
    /// tier would still fail. Downgrading everything to WARN instead would stop
    /// that oracle seeing genuine server faults, which is worse: it trades a
    /// noisy detector for a blind one.
    ///
    /// So the level follows the status. A 4xx is the caller's mistake and logs
    /// at WARN; a 5xx is ours and stays at ERROR, where the oracle can see it.
    ///
    /// `context` should name the operation -- ideally the database call -- so a
    /// log line still says where it came from now that the prose message is
    /// gone.
    /// Generic over the error type rather than taking `DatabaseError`, because
    /// the handlers do not all hold one. Most call `state.db.*` and get a
    /// `DatabaseError`; a few hold a bare `diesel::result::Error` or a
    /// `PoolError`. `ApiError` has a `From` for each, and every one of them
    /// classifies -- so the bound is "something ApiError already knows how to
    /// classify", which is exactly the set this should accept and no wider. An
    /// error with no `From` impl, such as `WebauthnError`, will not compile
    /// here, which is the correct answer: it is not part of this class and
    /// wants deciding on its own terms.
    pub fn from_db<E>(context: &str, err: E) -> Self
    where
        E: Display,
        Self: From<E>,
    {
        let detail = err.to_string();
        let api = Self::from(err);
        if api.is_server_error() {
            tracing::error!("{context}: {detail}");
        } else {
            tracing::warn!("{context}: {detail}");
        }
        api
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
            // Both branches invented this variant independently, for the same
            // reason: an insufficient role used to answer 401 Invalid token,
            // which the frontend interceptor reads as an expired session and
            // logs the user out -- so a member who touched an admin route was
            // silently signed out, and signing back in changed nothing. 403
            // says what actually happened.
            //
            // It is also the second conversion path for the same error, and it
            // has to agree with AuthError's own IntoResponse or a role
            // rejection means one thing when the extractor rejects and another
            // when a handler converts. That is precisely the divergence this
            // file was changed for once already; IntoResponse now delegates
            // here, so this arm is the only answer.
            AuthError::Forbidden(what) => {
                ApiError::Forbidden(format!("Insufficient permissions: {what} required"))
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
/// Does this Postgres message mean "the bytes you sent cannot be stored"?
///
/// Kept as a named function rather than inlined so the two phrases have one
/// home and the tests can reach them.
fn is_unrepresentable_text(message: &str) -> bool {
    message.contains("invalid byte sequence for encoding")
        || message.contains("has no equivalent in encoding")
}

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

            // Text the database cannot represent.
            //
            // Two distinct Postgres errors, one meaning: the bytes the caller
            // sent cannot be stored. `invalid byte sequence for encoding` is a
            // NUL inside a string, which no Postgres text column accepts in any
            // encoding; `has no equivalent in encoding` is a character outside
            // the cluster's own encoding, which is how emoji behave on LATIN1.
            //
            // Both were 500s. The first is a live defect on any deployment --
            // a request carrying %00 answered "the server broke" on a perfectly
            // ordinary UTF-8 database. The second is the defect TESTING.md
            // recorded as `astral-text-is-a-500-not-a-4xx`.
            //
            // Matched on the message text, which is not where anybody wants to
            // match. Diesel's `DatabaseErrorInformation` exposes no SQLSTATE, so
            // 22P05 and 22021 are not reachable as codes, and the alternative is
            // leaving both as 500. The strings are Postgres's own and stable in
            // English; a server whose messages are localised would fall through
            // to the arm below and answer 500, which is the previous behaviour
            // rather than a new failure. `errors.rs`'s own tests pin both
            // phrases so a silent change of wording is caught here rather than
            // in production.
            E::DatabaseError(Kind::Unknown, ref info)
                if is_unrepresentable_text(info.message()) =>
            {
                ApiError::BadRequest(
                    "Text contained characters this database cannot store".to_string(),
                )
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
    use super::is_unrepresentable_text;

    /// The two Postgres messages this classification depends on, verbatim.
    ///
    /// The comment on the matching arm claims these are pinned; this is that
    /// claim being kept. If Postgres rewords either, or somebody "tidies" the
    /// predicate, this fails here rather than silently returning 500s in
    /// production again.
    ///
    /// Copied from real server output captured on a LATIN1 cluster and on a
    /// UTF-8 one, not written from memory.
    #[test]
    fn the_two_messages_this_depends_on_are_recognised() {
        assert!(
            is_unrepresentable_text("invalid byte sequence for encoding \"UTF8\": 0x00"),
            "a NUL byte in text is refused by every Postgres encoding, so this \
             one fires on ordinary UTF-8 deployments"
        );
        assert!(
            is_unrepresentable_text(
                "character with byte sequence 0xf0 0x9f 0x90 0xb4 in encoding \
                 \"UTF8\" has no equivalent in encoding \"LATIN1\""
            ),
            "a character outside the cluster's encoding"
        );
    }

    /// Errors that are genuinely the server's problem must NOT be reclassified.
    ///
    /// The predicate is a substring match on prose, which is exactly the shape
    /// that quietly grows to swallow things it should not.
    #[test]
    fn unrelated_database_messages_are_left_alone() {
        for msg in [
            "could not connect to server: Connection refused",
            "deadlock detected",
            "out of shared memory",
            "canceling statement due to statement timeout",
            "duplicate key value violates unique constraint \"users_email_key\"",
        ] {
            assert!(
                !is_unrepresentable_text(msg),
                "{msg:?} is not a text-representation problem and must keep its \
                 own classification"
            );
        }
    }

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
        // Text the database cannot represent is now a 400, and this is the
        // record of that being deliberate.
        //
        // This assertion previously demanded 500 here, with the reasoning that
        // classifying it would mean matching English prose that changes with
        // the server's lc_messages -- true, and it was the right call while the
        // defect looked like a LATIN1-only curiosity. It is not: a NUL byte is
        // refused by every Postgres encoding, so the same arm answered 500 on
        // ordinary UTF-8 deployments. `is_unrepresentable_text` and its own
        // tests carry the reasoning and pin the two phrases.
        for message in [
            "character with byte sequence 0xf0 0x9f 0x9a 0xa7 in encoding \
             \"UTF8\" has no equivalent in encoding \"LATIN1\"",
            "invalid byte sequence for encoding \"UTF8\": 0x00",
        ] {
            assert_eq!(
                status_of(ApiError::from(db_error(Kind::Unknown, message))),
                StatusCode::BAD_REQUEST,
                "{message:?}"
            );
        }

        // The other half of the original assertion, kept: an Unknown that is
        // NOT a text-representation problem must stay ours. The predicate is a
        // substring match on prose, and this is what stops it growing to
        // swallow errors that really are the server's fault.
        for message in [
            "deadlock detected",
            "out of shared memory",
            "could not connect to server: Connection refused",
        ] {
            assert_eq!(
                status_of(ApiError::from(db_error(Kind::Unknown, message))),
                StatusCode::INTERNAL_SERVER_ERROR,
                "{message:?}"
            );
        }
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

    /// The test for the defect the stack battery found on its first full run.
    ///
    /// Every role gate returned `AuthError::InvalidToken` for an insufficient
    /// role, carrying a comment saying a Forbidden variant could be created.
    /// That is a 401, and `frontend/src/utils/api.ts:83` calls
    /// `authStore.logout()` on any 401 -- which is correct for an expired token
    /// and exactly wrong here. A Newbie who reached an admin-only endpoint was
    /// silently signed out, with no message, and signing back in changed
    /// nothing because the role had not changed either.
    ///
    /// Asserting the status is the narrow claim. The wider one is that 401 and
    /// 403 stay distinct: a client cannot tell "your session ended" from "you
    /// are not allowed" if the server says the same thing for both.
    #[test]
    fn an_insufficient_role_is_403_and_a_bad_token_is_401() {
        assert_eq!(
            status_of(ApiError::from(AuthError::Forbidden("administrator access"))),
            StatusCode::FORBIDDEN,
            "an authenticated caller who is not allowed must not be told to authenticate"
        );
        assert_eq!(
            status_of(ApiError::from(AuthError::InvalidToken)),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_of(ApiError::from(AuthError::MissingCredentials)),
            StatusCode::UNAUTHORIZED
        );
    }

    /// `AuthError` reaches a response by two routes, exactly as the diesel
    /// error does: its own `IntoResponse`, taken when an extractor rejects, and
    /// this `From` conversion, taken when a handler propagates one with `?`.
    ///
    /// The two are compared to **each other**, not to a table written here. A
    /// third hand-written copy of the mapping would be a third thing to keep in
    /// step, and it would agree with whichever of the two somebody updated
    /// while writing it. What matters is that a caller cannot tell which path
    /// their request took, so the two must not differ -- whatever they say.
    #[test]
    fn the_two_auth_conversion_paths_agree() {
        // Built twice rather than cloned: AuthError is not Clone, and
        // `into_response` consumes it.
        let build = || {
            vec![
                AuthError::WrongCredentials,
                AuthError::MissingCredentials,
                AuthError::InvalidToken,
                AuthError::Forbidden("administrator access"),
                AuthError::UserNotFound,
                AuthError::UserInactive,
                AuthError::TokenCreation,
                AuthError::InternalError,
                AuthError::InvalidPassword("too short".to_string()),
            ]
        };

        for (direct, converted) in build().into_iter().zip(build()) {
            let described = direct.to_string();
            let a = direct.into_response().status();
            let b = status_of(ApiError::from(converted));
            assert_eq!(
                a, b,
                "{described}: the extractor rejection answers {a} and the handler \
                 conversion answers {b}. Which one a caller gets depends on \
                 whether the error came out of an extractor or a `?`, which is \
                 not something any caller can know."
            );
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
