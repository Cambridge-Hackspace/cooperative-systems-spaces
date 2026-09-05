//! The cmi5 HTTP surface: admin/management, learner launch, and the embedded LRS.
//!
//! - Course import/list/get/delete (Staff) and the AU→training-step binding
//!   (Admin) that gates physical tool access.
//! - Launch (Member) and the one-time `fetch` exchange (public, token-gated).
//! - The LRS sub-routes under `/lrs` — statements and the State API — each
//!   authenticated only by `Cmi5SessionAuth` (the scoped session credential) and
//!   authorized per statement against that session's registration/actor/AU.
//!
//! The routes are always mounted; each handler checks whether the subsystem is
//! enabled in live config, so toggling `enabled` takes effect without a restart
//! and without changing the route table.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AdminUser, Cmi5SessionAuth, MemberUser, StaffUser};
use crate::cmi5::{Cmi5Error, GrantInfo};
use crate::models::{AuditEventType, Cmi5AssignableUnit, Cmi5Course, NewAuditLog};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

/// A hard ceiling on the import request body, independent of the configured
/// `max_package_bytes` (which the service enforces as the real limit). This only
/// keeps a hostile upload from being buffered without bound before the service
/// gets to check its size; it is deliberately well above any sane package.
const IMPORT_BODY_HARD_CAP: usize = 512 * 1024 * 1024;

pub fn cmi5_router() -> Router<AppState> {
    Router::new()
        .route(
            "/courses",
            post(import_course)
                .get(list_courses)
                // Raise the body limit for the upload route only; the global
                // /api nest keeps axum's small default.
                .layer(DefaultBodyLimit::max(IMPORT_BODY_HARD_CAP)),
        )
        .route(
            "/courses/{course_id}",
            get(get_course).delete(delete_course),
        )
        .route(
            "/courses/{course_id}/aus/{au_id}/assign",
            post(assign_au_step),
        )
        .route("/courses/{course_id}/export", get(export_course))
        // Learner discovery and launch (member), and the one-time fetch exchange
        // (public, but gated by the single-use token in its query string).
        .route("/modules", get(list_my_modules))
        .route("/aus/{au_id}/launch", post(launch_au))
        .route("/fetch", post(fetch_credential))
        // The embedded LRS: statements and the State API. Every handler here
        // authenticates only via Cmi5SessionAuth (the session credential), and
        // authorizes each write against that session's registration/actor/AU.
        .route(
            "/lrs/statements",
            put(lrs_put_statement)
                .post(lrs_post_statement)
                .get(lrs_get_statements),
        )
        .route(
            "/lrs/activities/state",
            get(lrs_get_state)
                .put(lrs_put_state)
                .delete(lrs_delete_state),
        )
}

/// A course together with its assignable units.
#[derive(Debug, Serialize)]
struct CourseWithAus {
    course: Cmi5Course,
    aus: Vec<Cmi5AssignableUnit>,
}

/// Body of the AU→step binding request. `training_step_id = null` unbinds.
#[derive(Debug, Deserialize)]
struct AssignAuRequest {
    training_step_id: Option<Uuid>,
}

/// Reject the request when the subsystem is turned off. Read from live config so
/// the toggle needs no restart.
fn ensure_enabled(state: &AppState) -> Result<(), ApiError> {
    if state.config_manager.get_config().cmi5.enabled {
        Ok(())
    } else {
        Err(ApiError::NotFound(
            "cmi5 support is not enabled on this instance".to_string(),
        ))
    }
}

async fn import_course(
    State(state): State<AppState>,
    staff: StaffUser,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;

    // Take the first file field. The client sends the .zip as a single part.
    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("malformed multipart upload: {e}")))?
    {
        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError::BadRequest(format!("could not read uploaded file: {e}")))?;
        bytes = Some(data.to_vec());
        break;
    }
    let bytes = bytes.ok_or_else(|| ApiError::BadRequest("no file in upload".to_string()))?;

    let course = state.cmi5_service.import_package(&bytes, staff.0.id)?;

    let audit = NewAuditLog {
        event_type: AuditEventType::Cmi5CoursePublished.as_str().to_string(),
        user_id: None,
        actor_id: Some(staff.0.id),
        event_data: serde_json::json!({
            "course_id": course.id,
            "course_iri": course.course_iri,
        }),
        ip_address: None,
        user_agent: None,
    };
    state.db.create_audit_log(&audit)?;

    let aus = state.cmi5_service.list_aus(course.id)?;
    Ok(ApiResponse::success(CourseWithAus { course, aus }))
}

async fn list_courses(
    State(state): State<AppState>,
    _staff: StaffUser,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;
    let courses = state.cmi5_service.list_courses()?;
    Ok(ApiResponse::success(courses))
}

async fn get_course(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(course_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;
    let course = state.cmi5_service.get_course(course_id)?;
    let aus = state.cmi5_service.list_aus(course.id)?;
    Ok(ApiResponse::success(CourseWithAus { course, aus }))
}

async fn delete_course(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(course_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;
    state.cmi5_service.delete_course(course_id)?;

    let audit = NewAuditLog {
        event_type: AuditEventType::Cmi5CourseDeleted.as_str().to_string(),
        user_id: None,
        actor_id: Some(staff.0.id),
        event_data: serde_json::json!({ "course_id": course_id }),
        ip_address: None,
        user_agent: None,
    };
    state.db.create_audit_log(&audit)?;

    Ok(ApiResponse::success(
        serde_json::json!({ "deleted": course_id }),
    ))
}

async fn assign_au_step(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((_course_id, au_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<AssignAuRequest>,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;
    let au = state
        .cmi5_service
        .assign_au_step(au_id, req.training_step_id)?;

    let audit = NewAuditLog {
        event_type: AuditEventType::Cmi5AuAssignedToTool.as_str().to_string(),
        user_id: None,
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "au_id": au_id,
            "training_step_id": req.training_step_id,
        }),
        ip_address: None,
        user_agent: None,
    };
    state.db.create_audit_log(&audit)?;

    Ok(ApiResponse::success(au))
}

async fn export_course(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(course_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let bytes = state.cmi5_service.export_package(course_id)?;

    let audit = NewAuditLog {
        event_type: AuditEventType::Cmi5CourseExported.as_str().to_string(),
        user_id: None,
        actor_id: Some(staff.0.id),
        event_data: serde_json::json!({ "course_id": course_id, "bytes": bytes.len() }),
        ip_address: None,
        user_agent: None,
    };
    state.db.create_audit_log(&audit)?;

    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"cmi5-{course_id}.zip\""),
        ),
    ];
    Ok((StatusCode::OK, headers, bytes).into_response())
}

/// Query string of the fetch endpoint: the one-time token lives here, exactly as
/// the LMS placed it in the launch URL.
#[derive(Debug, Deserialize)]
struct FetchQuery {
    token: Option<String>,
}

async fn list_my_modules(
    State(state): State<AppState>,
    member: MemberUser,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;
    let modules = state.cmi5_service.list_learner_modules(member.0.id)?;
    Ok(ApiResponse::success(modules))
}

async fn launch_au(
    State(state): State<AppState>,
    member: MemberUser,
    Path(au_id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    ensure_enabled(&state)?;
    let site_url = state.config_manager.get_config().site.site_url.clone();
    let result = state
        .cmi5_service
        .create_launch(au_id, member.0.id, &site_url)?;

    let audit = NewAuditLog {
        event_type: AuditEventType::Cmi5Launched.as_str().to_string(),
        user_id: Some(member.0.id),
        actor_id: Some(member.0.id),
        event_data: serde_json::json!({
            "au_id": au_id,
            "registration": result.registration_id,
        }),
        ip_address: None,
        user_agent: None,
    };
    state.db.create_audit_log(&audit)?;

    Ok(ApiResponse::success(serde_json::json!({
        "launch_url": result.launch_url,
        "registration": result.registration_id,
    })))
}

/// The cmi5 `fetch` endpoint. Public in the sense that no JWT extractor guards
/// it — the single-use token in the query string is the credential. The success
/// and error bodies are cmi5's own shapes (`auth-token` / `error-text`), not the
/// app envelope, because the caller is cmi5 content, not the SPA.
async fn fetch_credential(
    State(state): State<AppState>,
    Query(query): Query<FetchQuery>,
) -> Response {
    if !state.config_manager.get_config().cmi5.enabled {
        return fetch_error(StatusCode::NOT_FOUND, "cmi5 support is not enabled");
    }
    let Some(token) = query.token.filter(|t| !t.is_empty()) else {
        return fetch_error(StatusCode::BAD_REQUEST, "missing fetch token");
    };
    match state.cmi5_service.consume_fetch(&token) {
        Ok(session) => (
            StatusCode::OK,
            Json(serde_json::json!({ "auth-token": session })),
        )
            .into_response(),
        Err(_) => fetch_error(
            StatusCode::UNAUTHORIZED,
            "the fetch token is unknown, already used, or expired",
        ),
    }
}

/// A cmi5 fetch error body: `{"error-text": "..."}`, per the cmi5 spec.
fn fetch_error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error-text": message }))).into_response()
}

// ---------------------------------------------------------------------------
// The embedded LRS (Cmi5SessionAuth on every handler)
// ---------------------------------------------------------------------------

/// Statement endpoint query: the xAPI `statementId`, when the content PUTs.
#[derive(Debug, Deserialize)]
struct StatementQuery {
    #[serde(rename = "statementId")]
    statement_id: Option<Uuid>,
}

/// State API query. Only `stateId` is honored; the activity, agent, and
/// registration are taken from the authenticated session, not the query, so the
/// content cannot read or write another session's state.
#[derive(Debug, Deserialize)]
struct StateQuery {
    #[serde(rename = "stateId")]
    state_id: Option<String>,
}

/// On a grant, broadcast the new tool-access state to edge devices and record
/// the satisfaction in the audit log.
async fn apply_grant(state: &AppState, grant: GrantInfo) -> Result<(), ApiError> {
    crate::api::toolguard::broadcast_toolguard_state(state).await;
    let audit = NewAuditLog {
        event_type: AuditEventType::Cmi5AuSatisfied.as_str().to_string(),
        user_id: Some(grant.user_id),
        actor_id: Some(grant.user_id),
        event_data: serde_json::json!({
            "au_id": grant.au_id,
            "registration": grant.registration_id,
            "training_step_id": grant.training_step_id,
            "tool_id": grant.tool_id,
            "score": grant.score,
        }),
        ip_address: None,
        user_agent: None,
    };
    state.db.create_audit_log(&audit)?;
    Ok(())
}

async fn lrs_put_statement(
    State(state): State<AppState>,
    session: Cmi5SessionAuth,
    Query(query): Query<StatementQuery>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let site_url = state.config_manager.get_config().site.site_url.clone();
    if let Some(grant) =
        state
            .cmi5_service
            .record_statement(&session.0, &site_url, query.statement_id, body)?
    {
        apply_grant(&state, grant).await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn lrs_post_statement(
    State(state): State<AppState>,
    session: Cmi5SessionAuth,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let site_url = state.config_manager.get_config().site.site_url.clone();
    let items = match body {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    };
    let mut ids = Vec::new();
    for item in items {
        let sid = item
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        if let Some(grant) = state
            .cmi5_service
            .record_statement(&session.0, &site_url, sid, item)?
        {
            apply_grant(&state, grant).await?;
        }
        if let Some(sid) = sid {
            ids.push(sid.to_string());
        }
    }
    Ok((StatusCode::OK, Json(serde_json::json!(ids))).into_response())
}

async fn lrs_get_statements(
    State(state): State<AppState>,
    session: Cmi5SessionAuth,
    Query(query): Query<StatementQuery>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    match query.statement_id {
        Some(sid) => match state
            .cmi5_service
            .get_statement(session.0.registration_id, sid)?
        {
            Some(doc) => Ok((StatusCode::OK, Json(doc)).into_response()),
            None => Ok(StatusCode::NOT_FOUND.into_response()),
        },
        // A minimal StatementResult. cmi5 does not require the LMS to serve a
        // full statement query, and this session's own statements are the only
        // ones it could ever be shown.
        None => Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "statements": [], "more": "" })),
        )
            .into_response()),
    }
}

async fn lrs_get_state(
    State(state): State<AppState>,
    session: Cmi5SessionAuth,
    Query(query): Query<StateQuery>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let state_id = query
        .state_id
        .ok_or_else(|| ApiError::BadRequest("missing stateId".to_string()))?;
    match state.cmi5_service.get_state_document(
        session.0.registration_id,
        &session.0.activity_id,
        &session.0.user_id.to_string(),
        &state_id,
    )? {
        Some(doc) => Ok((StatusCode::OK, Json(doc)).into_response()),
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

async fn lrs_put_state(
    State(state): State<AppState>,
    session: Cmi5SessionAuth,
    Query(query): Query<StateQuery>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let state_id = query
        .state_id
        .ok_or_else(|| ApiError::BadRequest("missing stateId".to_string()))?;
    state.cmi5_service.put_state_document(
        session.0.registration_id,
        &session.0.activity_id,
        &session.0.user_id.to_string(),
        &state_id,
        body,
    )?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn lrs_delete_state(
    State(state): State<AppState>,
    session: Cmi5SessionAuth,
    Query(query): Query<StateQuery>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let state_id = query
        .state_id
        .ok_or_else(|| ApiError::BadRequest("missing stateId".to_string()))?;
    state.cmi5_service.delete_state_document(
        session.0.registration_id,
        &session.0.activity_id,
        &session.0.user_id.to_string(),
        &state_id,
    )?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

impl From<Cmi5Error> for ApiError {
    fn from(e: Cmi5Error) -> Self {
        match e {
            Cmi5Error::Disabled => {
                ApiError::NotFound("cmi5 support is not enabled on this instance".to_string())
            }
            Cmi5Error::TooLarge { size, max } => ApiError::BadRequest(format!(
                "package is {size} bytes, over the {max}-byte limit"
            )),
            Cmi5Error::Zip(m) => ApiError::BadRequest(format!("not a readable zip archive: {m}")),
            Cmi5Error::NoManifest => {
                ApiError::BadRequest("package has no cmi5.xml at its root".to_string())
            }
            Cmi5Error::Manifest(m) => ApiError::BadRequest(format!("invalid cmi5.xml: {m}")),
            Cmi5Error::ZipSlip(name) => ApiError::BadRequest(format!(
                "package entry '{name}' escapes the content directory"
            )),
            Cmi5Error::Io(m) => ApiError::InternalServerError(format!("filesystem error: {m}")),
            Cmi5Error::Pool(m) => {
                ApiError::InternalServerError(format!("database pool error: {m}"))
            }
            Cmi5Error::Db(err) => ApiError::from(err),
            Cmi5Error::CourseNotFound => ApiError::NotFound("no such cmi5 course".to_string()),
            Cmi5Error::AuNotFound => ApiError::NotFound("no such assignable unit".to_string()),
            Cmi5Error::StepNotFound => ApiError::NotFound("no such training step".to_string()),
            Cmi5Error::StepRequiresAssessment => ApiError::BadRequest(
                "a cmi5 module cannot satisfy a step that requires an assessment".to_string(),
            ),
            Cmi5Error::MoveOnNotApplicable => ApiError::BadRequest(
                "an AU with moveOn=NotApplicable can never satisfy, so it cannot gate a tool"
                    .to_string(),
            ),
            // The fetch endpoint answers in cmi5's own shape and does not use
            // this conversion; mapped here only to keep the match exhaustive.
            Cmi5Error::FetchTokenInvalid => ApiError::Unauthorized(
                "the fetch token is unknown, already used, or expired".to_string(),
            ),
            Cmi5Error::Json(m) => {
                ApiError::InternalServerError(format!("serialization error: {m}"))
            }
            Cmi5Error::BadStatement(m) => {
                ApiError::BadRequest(format!("malformed xAPI statement: {m}"))
            }
            // Identity/scope violations are 403 -- an authenticated session
            // reaching past what it may touch. Everything else (a malformed
            // result, a below-mastery pass, an out-of-order verb) is a 400: the
            // statement itself is wrong, not the caller's right to be here.
            Cmi5Error::Rejected(v) => {
                use ::cmi5::Violation;
                match v {
                    Violation::NotAnAccountActor
                    | Violation::ActorMismatch
                    | Violation::RegistrationMismatch
                    | Violation::ActivityMismatch => ApiError::Forbidden(v.to_string()),
                    _ => ApiError::BadRequest(v.to_string()),
                }
            }
        }
    }
}
