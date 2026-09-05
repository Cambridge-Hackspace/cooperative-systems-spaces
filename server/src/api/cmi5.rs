//! cmi5 admin/management HTTP surface (Stage 3): import a package, list/get/
//! delete courses, and bind an AU to a training step.
//!
//! Launch, the fetch handshake, and the LRS live in later stages. Every route
//! here is JWT-guarded (Staff for the course operations, Admin for the binding
//! that gates physical tool access). The routes are always mounted; each handler
//! checks whether the subsystem is enabled in live config, so toggling `enabled`
//! takes effect without a restart and without changing the route table.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AdminUser, StaffUser};
use crate::cmi5::Cmi5Error;
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
        }
    }
}
