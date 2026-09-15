//! Tool module bindings and safety interlocks (#83).
//!
//! Admin endpoints under `/api/admin/tool-modules` and
//! `/api/admin/tool-interlocks` author the wiring: which device fills a tool's
//! reader / power / sensor role, and which conditions gate or trip it. The
//! resulting snapshot is what the edge coordinates from.
//!
//! Nothing here energizes or de-energizes anything. Like [`crate::api::power`],
//! this is the map rather than the switch -- the enforcement lives on the edge
//! and, for the safety-critical rules, in the module firmware itself.
//!
//! Vocabularies (`role`, `on_disconnect`, interlock `kind` / `condition` /
//! `reset` / `enforcement`) are validated here against the same constants the
//! database CHECKs mirror, so a bad value is a 400 rather than a 500 from a
//! constraint violation.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::models::{
    enforcement, interlock_condition, interlock_kind, interlock_reset, module_role, on_disconnect,
    AuditEventType, NewAuditLog, NewToolInterlock, NewToolModule, ToolInterlock, ToolModule,
};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// Routers
// ---------------------------------------------------------------------------

/// Mounted at `/api/admin/tool-modules`.
pub fn admin_module_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_modules).post(create_module))
        .route("/state", get(get_module_state))
        .route("/{id}", axum::routing::delete(delete_module))
}

/// Mounted at `/api/admin/tool-interlocks`.
pub fn admin_interlock_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_interlocks).post(create_interlock))
        .route("/{id}", axum::routing::delete(delete_interlock))
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateModuleRequest {
    pub tool_id: Uuid,
    pub device_id: Uuid,
    pub role: String,
    pub name: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    pub on_disconnect: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInterlockRequest {
    pub tool_id: Uuid,
    pub kind: String,
    pub condition: String,
    #[serde(default)]
    pub source_module_id: Option<Uuid>,
    #[serde(default)]
    pub debounce_ms: Option<i32>,
    #[serde(default)]
    pub latch: Option<bool>,
    #[serde(default)]
    pub reset: Option<String>,
    #[serde(default)]
    pub enforcement: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DeletedResponse {
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Handlers: modules
// ---------------------------------------------------------------------------

async fn list_modules(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<ApiResponse<Vec<ToolModule>>>, ApiError> {
    let modules = state.db.list_tool_modules()?;
    Ok(Json(ApiResponse::success(modules)))
}

async fn create_module(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateModuleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let role = one_of(&req.role, &module_role::ALL, "role")?;
    let on_disc = match req.on_disconnect.as_deref() {
        Some(v) => one_of(v, &on_disconnect::ALL, "on_disconnect")?,
        // Deny-biased: an unstated policy is the fail-safe one, matching the
        // column default, so a binding created without one cannot hold a tool on
        // through a link loss.
        None => on_disconnect::FAIL_OFF.to_string(),
    };
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }

    // Resolve both FKs first so a missing tool or device is a 400 naming which
    // one, rather than a raw foreign-key 500 from the insert.
    if state.db.get_tool_by_id(req.tool_id)?.is_none() {
        return Err(ApiError::BadRequest("tool_id does not exist".to_string()));
    }
    if !state.db.space_device_exists(req.device_id)? {
        return Err(ApiError::BadRequest("device_id does not exist".to_string()));
    }

    let created = state.db.create_tool_module(&NewToolModule {
        tool_id: req.tool_id,
        device_id: req.device_id,
        role,
        name: req.name.trim().to_string(),
        params: req.params.unwrap_or_else(|| serde_json::json!({})),
        on_disconnect: on_disc,
    })?;

    audit(
        &state,
        AuditEventType::ToolModuleCreated,
        Some(admin.0.id),
        serde_json::json!({
            "module_id": created.id,
            "tool_id": created.tool_id,
            "device_id": created.device_id,
            "role": created.role,
            "name": created.name,
            "on_disconnect": created.on_disconnect,
        }),
    );
    broadcast(&state).await;

    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn delete_module(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeletedResponse>>, ApiError> {
    // The row count decides the status: deleting an id that matched nothing is a
    // 404, not a 200 plus an audit entry claiming a deletion that never happened.
    let affected = state.db.delete_tool_module(id)?;
    if affected == 0 {
        return Err(ApiError::NotFound("Tool module not found".to_string()));
    }

    audit(
        &state,
        AuditEventType::ToolModuleDeleted,
        Some(admin.0.id),
        serde_json::json!({ "module_id": id }),
    );
    broadcast(&state).await;

    Ok(Json(ApiResponse::success(DeletedResponse {
        deleted: true,
    })))
}

/// The snapshot the edge coordinates from, as the admin surface sees it.
async fn get_module_state(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<css_lib::wire::ToolModuleStatePayload>, ApiError> {
    Ok(Json(state.db.module_state_snapshot()?))
}

// ---------------------------------------------------------------------------
// Handlers: interlocks
// ---------------------------------------------------------------------------

async fn list_interlocks(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<ApiResponse<Vec<ToolInterlock>>>, ApiError> {
    let interlocks = state.db.list_tool_interlocks()?;
    Ok(Json(ApiResponse::success(interlocks)))
}

async fn create_interlock(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateInterlockRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let kind = one_of(&req.kind, &interlock_kind::ALL, "kind")?;
    let condition = one_of(&req.condition, &interlock_condition::ALL, "condition")?;
    let reset = match req.reset.as_deref() {
        Some(v) => one_of(v, &interlock_reset::ALL, "reset")?,
        None => interlock_reset::RE_AUTH.to_string(),
    };
    let enforce = match req.enforcement.as_deref() {
        Some(v) => one_of(v, &enforcement::ALL, "enforcement")?,
        None => enforcement::EDGE.to_string(),
    };
    let debounce_ms = req.debounce_ms.unwrap_or(0);
    if debounce_ms < 0 {
        return Err(ApiError::BadRequest(
            "debounce_ms must not be negative".to_string(),
        ));
    }

    if state.db.get_tool_by_id(req.tool_id)?.is_none() {
        return Err(ApiError::BadRequest("tool_id does not exist".to_string()));
    }

    // A rule whose source module belongs to a different tool would read a sensor
    // on the wrong machine -- worth refusing rather than discovering in a
    // workshop.
    if let Some(src) = req.source_module_id {
        let modules = state.db.list_tool_modules_for_tool(req.tool_id)?;
        if !modules.iter().any(|m| m.id == src) {
            return Err(ApiError::BadRequest(
                "source_module_id is not a module of this tool".to_string(),
            ));
        }
    }

    let created = state.db.create_tool_interlock(&NewToolInterlock {
        tool_id: req.tool_id,
        kind,
        condition,
        source_module_id: req.source_module_id,
        debounce_ms,
        // A trip latches unless explicitly told otherwise: auto-resuming a cut
        // tool when the condition clears is how a laser restarts under an open
        // lid. A start gate has nothing to latch.
        latch: req.latch.unwrap_or(true),
        reset,
        enforcement: enforce,
        enabled: req.enabled.unwrap_or(true),
    })?;

    audit(
        &state,
        AuditEventType::ToolInterlockCreated,
        Some(admin.0.id),
        serde_json::json!({
            "interlock_id": created.id,
            "tool_id": created.tool_id,
            "kind": created.kind,
            "condition": created.condition,
            "source_module_id": created.source_module_id,
            "latch": created.latch,
            "reset": created.reset,
            "enforcement": created.enforcement,
            "enabled": created.enabled,
        }),
    );
    broadcast(&state).await;

    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn delete_interlock(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeletedResponse>>, ApiError> {
    let affected = state.db.delete_tool_interlock(id)?;
    if affected == 0 {
        return Err(ApiError::NotFound("Tool interlock not found".to_string()));
    }

    audit(
        &state,
        AuditEventType::ToolInterlockDeleted,
        Some(admin.0.id),
        serde_json::json!({ "interlock_id": id }),
    );
    broadcast(&state).await;

    Ok(Json(ApiResponse::success(DeletedResponse {
        deleted: true,
    })))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Accept `value` only if it is in `allowed`, naming the field and the permitted
/// set on rejection. The database CHECKs the same sets; catching it here makes it
/// a 400 that says what was wrong instead of a 500 from a constraint.
fn one_of(value: &str, allowed: &[&str], field: &str) -> Result<String, ApiError> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ApiError::BadRequest(format!(
            "{field} must be one of: {}",
            allowed.join(", ")
        )))
    }
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
        tracing::error!(
            "Failed to write tool module audit log {}: {}",
            event.as_str(),
            e
        );
    }
}

/// Push the new snapshot after any change, so an edge's cached wiring cannot
/// outlive the rule that produced it.
async fn broadcast(state: &AppState) {
    crate::api::toolguard::broadcast_module_state(state).await;
}
