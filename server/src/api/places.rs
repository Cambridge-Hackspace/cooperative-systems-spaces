//! Configurable hierarchy of physical places.
//!
//! Admin endpoints under `/api/admin/places` manage the tree; member-facing
//! endpoints under `/api/places` expose a read-only directory view. Level
//! names come from `[place].types` in config — see
//! [`crate::config::PlaceConfig`].

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AdminUser, AuthUser};
use crate::database::DatabaseError;
use crate::models::{AuditEventType, NewAuditLog, NewPlace, Place, UpdatePlace};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// Routers
// ---------------------------------------------------------------------------

pub fn member_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_places_member))
        .route("/{id}", get(get_place_member))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config))
        .route("/", get(list_places_admin).post(create_place))
        .route(
            "/{id}",
            get(get_place_admin)
                .patch(update_place)
                .delete(delete_place),
        )
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct PlaceConfigResponse {
    pub enabled: bool,
    pub types: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaceRequest {
    pub name: String,
    pub place_type: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    /// Mark this as a "special" place (e.g. Outside, Common Area). Special
    /// places must be roots and bypass the configured type-ordering rules.
    #[serde(default)]
    pub is_special: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdatePlaceRequest {
    pub name: Option<String>,
    pub place_type: Option<String>,
    /// Outer `Some` = field present in request; inner `None` = promote to root.
    #[serde(default)]
    pub parent_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub external_id: Option<Option<String>>,
    #[serde(default)]
    pub is_special: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PlaceDetailResponse {
    #[serde(flatten)]
    pub place: Place,
    /// Ordered top-down: `[root, …, immediate_parent]`.
    pub ancestors: Vec<Place>,
    pub children: Vec<Place>,
    pub attached: AttachedCounts,
}

#[derive(Debug, Serialize)]
pub struct AttachedCounts {
    pub doors: i64,
    pub tools: i64,
    pub devices: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().place.enabled {
        return Err(ApiError::Forbidden(
            "Places module is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

fn audit(state: &AppState, event: AuditEventType, actor: Option<Uuid>, data: serde_json::Value) {
    let log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: actor,
        actor_id: actor,
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write place audit log {}: {}", event.as_str(), e);
    }
}

/// Validate that `place_type` exists in the configured vocabulary and that,
/// if `parent_id` is set, the child is at a strictly deeper level than its
/// parent. Special places skip both checks: their type is free-form and
/// they must be roots.
fn validate_place_payload(
    state: &AppState,
    place_type: &str,
    parent_id: Option<Uuid>,
    is_special: bool,
) -> Result<(), ApiError> {
    if is_special {
        if parent_id.is_some() {
            return Err(ApiError::BadRequest(
                "Special places cannot have a parent".to_string(),
            ));
        }
        if place_type.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "place_type is required (use any label, e.g. \"Outside\")".to_string(),
            ));
        }
        return Ok(());
    }
    let cfg = state.config_manager.get_config();
    if cfg.place.index_of(place_type).is_none() {
        return Err(ApiError::BadRequest(format!(
            "Unknown place_type '{place_type}'; configured types: {:?}",
            cfg.place.types
        )));
    }
    if let Some(parent_id) = parent_id {
        let parent = state.db.get_place(parent_id).map_err(ApiError::from)?;
        if parent.is_special {
            // Children of special places use the regular hierarchy from here on.
            // (Allowed — e.g. `Outside → Parking Lot A`.) Skip the type-deeper
            // check because the parent has no depth index.
            return Ok(());
        }
        cfg.place
            .validate_parent_child(&parent.place_type, place_type)
            .map_err(ApiError::BadRequest)?;
    }
    Ok(())
}

fn map_diesel_conflict(e: DatabaseError) -> ApiError {
    match e {
        DatabaseError::Diesel(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => ApiError::Conflict("A sibling place with that name already exists".to_string()),
        DatabaseError::Diesel(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::ForeignKeyViolation,
            _,
        )) => ApiError::Conflict(
            "This place still has child places; move or delete them first".to_string(),
        ),
        other => ApiError::from(other),
    }
}

// ---------------------------------------------------------------------------
// Admin handlers
// ---------------------------------------------------------------------------

async fn get_config(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let cfg = state.config_manager.get_config();
    Ok(Json(ApiResponse::success(PlaceConfigResponse {
        enabled: cfg.place.enabled,
        types: cfg.place.types.clone(),
    })))
}

async fn list_places_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let places = state.db.list_places()?;
    Ok(Json(ApiResponse::success(places)))
}

async fn get_place_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let place = state.db.get_place(id)?;
    let ancestors = state.db.place_ancestors(id)?;
    let children = state.db.list_child_places(id)?;
    let (doors, tools, devices) = state.db.count_place_references(id)?;
    Ok(Json(ApiResponse::success(PlaceDetailResponse {
        place,
        ancestors,
        children,
        attached: AttachedCounts {
            doors,
            tools,
            devices,
        },
    })))
}

async fn create_place(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreatePlaceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    validate_place_payload(&state, &req.place_type, req.parent_id, req.is_special)?;

    let created = state
        .db
        .create_place(&NewPlace {
            parent_id: req.parent_id,
            place_type: req.place_type,
            name: req.name.trim().to_string(),
            description: req.description,
            external_id: req.external_id,
            is_special: req.is_special,
        })
        .map_err(map_diesel_conflict)?;

    audit(
        &state,
        AuditEventType::PlaceCreated,
        Some(admin.0.id),
        serde_json::json!({
            "place_id": created.id,
            "name": created.name,
            "place_type": created.place_type,
            "parent_id": created.parent_id,
        }),
    );

    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn update_place(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePlaceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let before = state.db.get_place(id)?;

    // Compute prospective new type + parent + specialness so we can validate.
    let new_type = req
        .place_type
        .clone()
        .unwrap_or_else(|| before.place_type.clone());
    let new_parent: Option<Uuid> = match &req.parent_id {
        Some(opt) => *opt,
        None => before.parent_id,
    };
    let new_is_special = req.is_special.unwrap_or(before.is_special);

    // Self-parent and cycle checks: only matter when the parent_id is changing.
    if let Some(parent) = new_parent {
        if parent == id {
            return Err(ApiError::BadRequest(
                "A place can't be its own parent".to_string(),
            ));
        }
        // Walk the prospective parent's ancestors; if `id` appears we'd
        // create a cycle. (Walks bounded by `place_ancestors`.)
        let chain = state.db.place_ancestors(parent)?;
        if chain.iter().any(|p| p.id == id) {
            return Err(ApiError::BadRequest(
                "Move would create a cycle in the place hierarchy".to_string(),
            ));
        }
    }

    validate_place_payload(&state, &new_type, new_parent, new_is_special)?;

    let changes = UpdatePlace {
        parent_id: req.parent_id,
        place_type: req.place_type,
        name: req.name.map(|n| n.trim().to_string()),
        description: req.description,
        external_id: req.external_id,
        updated_at: Some(Utc::now()),
        is_special: req.is_special,
    };

    let after = state
        .db
        .update_place(id, &changes)
        .map_err(map_diesel_conflict)?;

    audit(
        &state,
        AuditEventType::PlaceUpdated,
        Some(admin.0.id),
        serde_json::json!({ "place_id": id }),
    );
    if before.parent_id != after.parent_id {
        audit(
            &state,
            AuditEventType::PlaceMoved,
            Some(admin.0.id),
            serde_json::json!({
                "place_id": id,
                "old_parent_id": before.parent_id,
                "new_parent_id": after.parent_id,
            }),
        );
    }

    Ok(Json(ApiResponse::success(after)))
}

async fn delete_place(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let prev = state.db.get_place(id)?;
    let deleted = state.db.delete_place(id).map_err(map_diesel_conflict)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Place not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::PlaceDeleted,
        Some(admin.0.id),
        serde_json::json!({ "place_id": id, "name": prev.name }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Place deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Member-facing handlers (read-only directory view)
// ---------------------------------------------------------------------------

async fn list_places_member(
    State(state): State<AppState>,
    _user: AuthUser,
) -> Result<impl IntoResponse, ApiError> {
    let places = state.db.list_places()?;
    Ok(Json(ApiResponse::success(places)))
}

async fn get_place_member(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let place = state.db.get_place(id)?;
    let ancestors = state.db.place_ancestors(id)?;
    Ok(Json(ApiResponse::success(serde_json::json!({
        "place": place,
        "ancestors": ancestors,
    }))))
}
