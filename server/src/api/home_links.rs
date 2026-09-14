//! Admin-curated links displayed on the public home page.
//!
//! Two surfaces:
//!   * `/api/admin/home-links` — admin CRUD.
//!   * `/api/public/home-links` — no auth required; the handler inspects any
//!     bearer token to learn the requester's role and filters the result so
//!     `anonymous` links go to logged-out visitors only, role-gated links go
//!     to qualifying users, and `everyone` is always shown.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AdminUser, AuthService};
use crate::models::{
    AuditEventType, HomeLink, HomeLinkAudience, NewAuditLog, NewHomeLink, UpdateHomeLink,
};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// Routers
// ---------------------------------------------------------------------------

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_links_admin).post(create_link))
        .route(
            "/{id}",
            get(get_link_admin).patch(update_link).delete(delete_link),
        )
}

pub fn public_routes() -> Router<AppState> {
    Router::new().route("/home-links", get(list_links_public))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateHomeLinkRequest {
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub audience: HomeLinkAudience,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional RFC-3339 timestamp. After this moment, the link is hidden
    /// from the public endpoint (admin still sees it as **expired**).
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<Utc>>,
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateHomeLinkRequest {
    pub label: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub icon: Option<Option<String>>,
    pub audience: Option<HomeLinkAudience>,
    pub sort_order: Option<i32>,
    pub enabled: Option<bool>,
    /// Outer `Some` = field present in request; inner `None` = clear the expiry.
    #[serde(default)]
    pub expires_at: Option<Option<chrono::DateTime<Utc>>>,
}

#[derive(Debug, Serialize)]
pub struct HomeLinkResponse {
    pub id: Uuid,
    pub label: String,
    pub url: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub audience: HomeLinkAudience,
    pub sort_order: i32,
    pub enabled: bool,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

impl HomeLinkResponse {
    fn from_row(row: HomeLink) -> Self {
        let audience = HomeLinkAudience::parse(&row.audience).unwrap_or(HomeLinkAudience::Everyone);
        Self {
            id: row.id,
            label: row.label,
            url: row.url,
            description: row.description,
            icon: row.icon,
            audience,
            sort_order: row.sort_order,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
            expires_at: row.expires_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
            "Failed to write home_link audit log {}: {}",
            event.as_str(),
            e
        );
    }
}

/// True iff a link with `audience` should be shown to a viewer whose effective
/// tier `level` is `viewer_level` (None = signed-out). The `Member`/`Staff`
/// audiences gate on the tier levels resolved from the RBAC graph, replacing the
/// legacy `UserRole::rank()` comparison.
fn visible_to(
    audience: HomeLinkAudience,
    viewer_level: Option<i16>,
    member_level: i16,
    staff_level: i16,
) -> bool {
    match (audience, viewer_level) {
        (HomeLinkAudience::Everyone, _) => true,
        (HomeLinkAudience::Anonymous, None) => true,
        (HomeLinkAudience::Anonymous, Some(_)) => false,
        (HomeLinkAudience::LoggedIn, Some(_)) => true,
        (HomeLinkAudience::LoggedIn, None) => false,
        (HomeLinkAudience::Member, Some(l)) => l >= member_level,
        (HomeLinkAudience::Staff, Some(l)) => l >= staff_level,
        (HomeLinkAudience::Member | HomeLinkAudience::Staff, None) => false,
    }
}

/// Best-effort: parse an `Authorization: Bearer <jwt>` header and resolve the
/// caller's effective tier level. Returns `None` for unauthenticated / invalid
/// tokens, which is the same behavior we want from the gate's perspective.
fn role_level_from_headers(state: &AppState, headers: &HeaderMap) -> Option<i16> {
    let auth = headers.get("authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    let cfg = state.config_manager.get_config();
    let svc = AuthService::new(&state.db, &cfg.auth.jwt_secret);
    let user = svc.get_user_from_token(token).ok()?;
    state.db.user_effective_level(user.id).ok()
}

// ---------------------------------------------------------------------------
// Public handler — audience-filtered
// ---------------------------------------------------------------------------

async fn list_links_public(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let viewer_level = role_level_from_headers(&state, &headers);
    // Resolve the audience thresholds once from the graph (the "Member" audience
    // means the member tier and up; "Staff" the staff tier and up).
    let graph = state.db.rbac();
    let member_level = graph.level_of_name("member").unwrap_or(i16::MAX);
    let staff_level = graph.level_of_name("staff").unwrap_or(i16::MAX);
    let now = chrono::Utc::now();
    let rows = state.db.list_home_links()?;
    let resp: Vec<HomeLinkResponse> = rows
        .into_iter()
        .filter(|r| r.enabled)
        // Drop rows whose expiry has passed. NULL expiry = never expires.
        .filter(|r| r.expires_at.map(|t| t > now).unwrap_or(true))
        .filter_map(|r| {
            let audience = HomeLinkAudience::parse(&r.audience)?;
            if visible_to(audience, viewer_level, member_level, staff_level) {
                Some(HomeLinkResponse::from_row(r))
            } else {
                None
            }
        })
        .collect();
    Ok(Json(ApiResponse::success(resp)))
}

// ---------------------------------------------------------------------------
// Admin handlers
// ---------------------------------------------------------------------------

async fn list_links_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let rows = state.db.list_home_links()?;
    let resp: Vec<HomeLinkResponse> = rows.into_iter().map(HomeLinkResponse::from_row).collect();
    Ok(Json(ApiResponse::success(resp)))
}

async fn get_link_admin(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let row = state.db.get_home_link(id)?;
    Ok(Json(ApiResponse::success(HomeLinkResponse::from_row(row))))
}

async fn create_link(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateHomeLinkRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.label.trim().is_empty() {
        return Err(ApiError::BadRequest("label is required".to_string()));
    }
    if req.url.trim().is_empty() {
        return Err(ApiError::BadRequest("url is required".to_string()));
    }
    let row = state.db.create_home_link(&NewHomeLink {
        label: req.label.trim().to_string(),
        url: req.url.trim().to_string(),
        description: req.description,
        icon: req.icon,
        audience: req.audience.as_str().to_string(),
        sort_order: req.sort_order,
        enabled: req.enabled,
        created_by: Some(admin.0.id),
        expires_at: req.expires_at,
    })?;
    audit(
        &state,
        AuditEventType::HomeLinkCreated,
        Some(admin.0.id),
        serde_json::json!({ "home_link_id": row.id, "label": row.label, "audience": row.audience }),
    );
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(HomeLinkResponse::from_row(row))),
    ))
}

async fn update_link(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateHomeLinkRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let row = state.db.update_home_link(
        id,
        &UpdateHomeLink {
            label: req.label.map(|s| s.trim().to_string()),
            url: req.url.map(|s| s.trim().to_string()),
            description: req.description,
            icon: req.icon,
            audience: req.audience.map(|a| a.as_str().to_string()),
            sort_order: req.sort_order,
            enabled: req.enabled,
            updated_at: Some(Utc::now()),
            expires_at: req.expires_at,
        },
    )?;
    audit(
        &state,
        AuditEventType::HomeLinkUpdated,
        Some(admin.0.id),
        serde_json::json!({ "home_link_id": id }),
    );
    Ok(Json(ApiResponse::success(HomeLinkResponse::from_row(row))))
}

async fn delete_link(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let prev = state.db.get_home_link(id)?;
    let n = state.db.delete_home_link(id)?;
    if n == 0 {
        return Err(ApiError::NotFound("Home link not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::HomeLinkDeleted,
        Some(admin.0.id),
        serde_json::json!({ "home_link_id": id, "label": prev.label }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Home link deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Tests for the small visibility predicate.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // The seeded tier levels the graph resolves: newbie 1, member 2, staff 3,
    // admin 4. The audience thresholds are member_level=2, staff_level=3.
    const NEWBIE: i16 = 1;
    const MEMBER: i16 = 2;
    const STAFF: i16 = 3;
    const ADMIN: i16 = 4;
    const MEMBER_LEVEL: i16 = 2;
    const STAFF_LEVEL: i16 = 3;

    fn vis(audience: HomeLinkAudience, viewer_level: Option<i16>) -> bool {
        visible_to(audience, viewer_level, MEMBER_LEVEL, STAFF_LEVEL)
    }

    #[test]
    fn everyone_sees_everyone() {
        assert!(vis(HomeLinkAudience::Everyone, None));
        assert!(vis(HomeLinkAudience::Everyone, Some(MEMBER)));
    }

    #[test]
    fn anonymous_only_when_signed_out() {
        assert!(vis(HomeLinkAudience::Anonymous, None));
        assert!(!vis(HomeLinkAudience::Anonymous, Some(NEWBIE)));
    }

    #[test]
    fn logged_in_hidden_when_signed_out() {
        assert!(!vis(HomeLinkAudience::LoggedIn, None));
        assert!(vis(HomeLinkAudience::LoggedIn, Some(NEWBIE)));
    }

    #[test]
    fn member_gate_respects_hierarchy() {
        assert!(!vis(HomeLinkAudience::Member, Some(NEWBIE)));
        assert!(vis(HomeLinkAudience::Member, Some(MEMBER)));
        assert!(vis(HomeLinkAudience::Member, Some(STAFF)));
        assert!(vis(HomeLinkAudience::Member, Some(ADMIN)));
    }

    #[test]
    fn staff_gate_respects_hierarchy() {
        assert!(!vis(HomeLinkAudience::Staff, Some(MEMBER)));
        assert!(vis(HomeLinkAudience::Staff, Some(STAFF)));
        assert!(vis(HomeLinkAudience::Staff, Some(ADMIN)));
    }
}
