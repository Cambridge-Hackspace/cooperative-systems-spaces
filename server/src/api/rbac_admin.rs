//! Admin RBAC management API (#65 Phase 3).
//!
//! Mounted at `/api/admin/rbac`. Reads the whole configuration and edits roles,
//! the role x permission matrix, and the inheritance graph. User<->role
//! assignment lives under `/api/admin/users/{id}/roles` (in `admin.rs`, next to
//! the other user operations). Every mutation is audited and, where it changes
//! the resolver's inputs (roles / grants / inheritance), rebuilds the cached
//! graph so enforcement sees the change immediately.
//!
//! Policy lives here rather than in the database layer so it can answer a 4xx:
//! system roles are protected from edit/delete, permission keys are validated
//! against the catalog, and an inheritance edit that would create a cycle is
//! refused.

use axum::{
    extract::{Path, State},
    response::Json,
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::AdminUser,
    models::AuditEventType,
    rbac::RoleGraph,
    AppState,
};

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(get_rbac))
        .route("/roles", post(create_role))
        .route(
            "/roles/{role_id}",
            axum::routing::patch(update_role).delete(delete_role),
        )
        .route("/roles/{role_id}/permissions", put(set_role_permissions))
        .route("/roles/{role_id}/inheritance", put(set_role_inheritance))
}

// ---- Views (read) ----------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct RoleView {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub is_system: bool,
    pub level: i16,
    /// Role ids this role inherits from directly.
    pub inherits: Vec<Uuid>,
    /// Permission keys granted directly to this role (not counting inheritance).
    pub permissions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PermissionView {
    pub key: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct RbacView {
    pub roles: Vec<RoleView>,
    pub permissions: Vec<PermissionView>,
}

/// `GET /api/admin/rbac` — the whole RBAC configuration.
pub async fn get_rbac(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<RbacView>>, ApiError> {
    let (roles, permissions, grants, edges) = state.db.rbac_snapshot().map_err(ApiError::from)?;

    let mut own: std::collections::HashMap<Uuid, Vec<String>> = std::collections::HashMap::new();
    for (role_id, key) in grants {
        own.entry(role_id).or_default().push(key);
    }
    let mut inherits: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();
    for (role_id, parent) in edges {
        inherits.entry(role_id).or_default().push(parent);
    }

    let roles = roles
        .into_iter()
        .map(|r| {
            let mut permissions = own.remove(&r.id).unwrap_or_default();
            permissions.sort();
            RoleView {
                id: r.id,
                name: r.name,
                description: r.description,
                is_system: r.is_system,
                level: r.level,
                inherits: inherits.remove(&r.id).unwrap_or_default(),
                permissions,
            }
        })
        .collect();

    let permissions = permissions
        .into_iter()
        .map(|(key, description)| PermissionView { key, description })
        .collect();

    Ok(Json(ApiResponse::success(RbacView { roles, permissions })))
}

// ---- Roles (write) ---------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub level: i16,
}

#[derive(Debug, Serialize)]
pub struct RoleDetail {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub is_system: bool,
    pub level: i16,
}

impl From<crate::rbac::Role> for RoleDetail {
    fn from(r: crate::rbac::Role) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            is_system: r.is_system,
            level: r.level,
        }
    }
}

async fn audit(
    state: &AppState,
    event: AuditEventType,
    subject: Option<Uuid>,
    actor: Uuid,
    detail: serde_json::Value,
) {
    if let Err(e) = state
        .audit_logger
        .log_event(event, subject, Some(actor), detail, None, None)
        .await
    {
        tracing::warn!("failed to write RBAC audit event: {e}");
    }
}

/// `POST /api/admin/rbac/roles` — create a custom role.
pub async fn create_role(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateRoleRequest>,
) -> Result<Json<ApiResponse<RoleDetail>>, ApiError> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "role name must not be empty".to_string(),
        ));
    }
    if payload.level < 0 {
        return Err(ApiError::BadRequest(
            "role level must not be negative".to_string(),
        ));
    }
    // A duplicate name is a 409 through ApiError::from(diesel unique violation).
    let role = state
        .db
        .create_role(name, payload.description.trim(), payload.level)
        .map_err(ApiError::from)?;
    audit(
        &state,
        AuditEventType::RoleCreated,
        None,
        admin.0.id,
        serde_json::json!({ "role_id": role.id, "name": role.name, "level": role.level }),
    )
    .await;
    Ok(Json(ApiResponse::success(RoleDetail::from(role))))
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub description: Option<String>,
    pub level: Option<i16>,
}

/// `PATCH /api/admin/rbac/roles/{role_id}` — edit description and/or level.
pub async fn update_role(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<UpdateRoleRequest>,
) -> Result<Json<ApiResponse<RoleDetail>>, ApiError> {
    let existing = state
        .db
        .get_role(role_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("role not found".to_string()))?;
    // A system role's level anchors the legacy tier ladder; only its description
    // may be edited.
    if existing.is_system && payload.level.is_some() {
        return Err(ApiError::Forbidden(
            "the level of a system role cannot be changed".to_string(),
        ));
    }
    if let Some(l) = payload.level {
        if l < 0 {
            return Err(ApiError::BadRequest(
                "role level must not be negative".to_string(),
            ));
        }
    }
    let role = state
        .db
        .update_role(role_id, payload.description, payload.level)
        .map_err(ApiError::from)?;
    audit(
        &state,
        AuditEventType::RoleUpdated,
        None,
        admin.0.id,
        serde_json::json!({ "role_id": role.id, "name": role.name, "level": role.level }),
    )
    .await;
    Ok(Json(ApiResponse::success(RoleDetail::from(role))))
}

/// `DELETE /api/admin/rbac/roles/{role_id}` — delete a custom role.
pub async fn delete_role(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let existing = state
        .db
        .get_role(role_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("role not found".to_string()))?;
    if existing.is_system {
        return Err(ApiError::Forbidden(
            "system roles cannot be deleted".to_string(),
        ));
    }
    state.db.delete_role(role_id).map_err(ApiError::from)?;
    audit(
        &state,
        AuditEventType::RoleDeleted,
        None,
        admin.0.id,
        serde_json::json!({ "role_id": role_id, "name": existing.name }),
    )
    .await;
    Ok(Json(ApiResponse::success(())))
}

// ---- Matrix + inheritance (write) ------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SetPermissionsRequest {
    /// The complete set of permission keys granted directly to the role.
    pub permissions: Vec<String>,
}

/// `PUT /api/admin/rbac/roles/{role_id}/permissions` — replace the role's grants.
pub async fn set_role_permissions(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<SetPermissionsRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    state
        .db
        .get_role(role_id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("role not found".to_string()))?;

    // Every key must be in the catalog -- a grant of a permission nothing checks
    // is inert and almost certainly a typo.
    let catalog: std::collections::HashSet<String> = state
        .db
        .permission_keys()
        .map_err(ApiError::from)?
        .into_iter()
        .collect();
    let mut keys = payload.permissions.clone();
    keys.sort();
    keys.dedup();
    if let Some(unknown) = keys.iter().find(|k| !catalog.contains(*k)) {
        return Err(ApiError::BadRequest(format!(
            "unknown permission key: {unknown}"
        )));
    }

    state
        .db
        .set_role_permissions(role_id, &keys)
        .map_err(ApiError::from)?;
    audit(
        &state,
        AuditEventType::RolePermissionsChanged,
        None,
        admin.0.id,
        serde_json::json!({ "role_id": role_id, "permissions": keys }),
    )
    .await;
    Ok(Json(ApiResponse::success(())))
}

#[derive(Debug, Deserialize)]
pub struct SetInheritanceRequest {
    /// The complete set of role ids this role inherits from directly.
    pub inherits: Vec<Uuid>,
}

/// `PUT /api/admin/rbac/roles/{role_id}/inheritance` — replace inheritance edges.
pub async fn set_role_inheritance(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<SetInheritanceRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let (roles, _permissions, _grants, edges) = state.db.rbac_snapshot().map_err(ApiError::from)?;
    let valid_ids: std::collections::HashSet<Uuid> = roles.iter().map(|r| r.id).collect();
    if !valid_ids.contains(&role_id) {
        return Err(ApiError::NotFound("role not found".to_string()));
    }

    let mut parents = payload.inherits.clone();
    parents.sort();
    parents.dedup();
    if parents.contains(&role_id) {
        return Err(ApiError::BadRequest(
            "a role cannot inherit from itself".to_string(),
        ));
    }
    if let Some(unknown) = parents.iter().find(|p| !valid_ids.contains(*p)) {
        return Err(ApiError::BadRequest(format!(
            "unknown role id in inheritance: {unknown}"
        )));
    }

    // Build the graph as it would be after this edit and refuse a cycle.
    let role_rows: Vec<(Uuid, String, i16)> = roles
        .iter()
        .map(|r| (r.id, r.name.clone(), r.level))
        .collect();
    let mut edge_rows: Vec<(Uuid, Uuid)> = edges
        .into_iter()
        .filter(|(rid, _)| *rid != role_id)
        .collect();
    for parent in &parents {
        edge_rows.push((role_id, *parent));
    }
    let candidate = RoleGraph::from_rows(&role_rows, &[], &edge_rows);
    if candidate.has_cycle() {
        return Err(ApiError::BadRequest(
            "this inheritance change would create a cycle".to_string(),
        ));
    }

    state
        .db
        .set_role_inheritance(role_id, &parents)
        .map_err(ApiError::from)?;
    audit(
        &state,
        AuditEventType::RoleInheritanceChanged,
        None,
        admin.0.id,
        serde_json::json!({ "role_id": role_id, "inherits": parents }),
    )
    .await;
    Ok(Json(ApiResponse::success(())))
}
