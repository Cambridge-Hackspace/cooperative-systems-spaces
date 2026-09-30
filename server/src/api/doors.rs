//! Door access HTTP API.
//!
//! - Member-facing routes (`/api/doors/*`) require any authenticated user and
//!   drive the QR "I'm here" check-in flow.
//! - Admin routes (`/api/admin/doors/*`) require AdminUser and manage doors,
//!   per-door access rules, events, and remote unlocks.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{AdminUser, AuthUser};
use crate::doors::AccessDecision;
use crate::models::{
    AccessRule, AuditEventType, Door, DoorAccessEvent, DoorAccessMethod, DoorRuleEffect,
    DoorRuleKind, NewAccessRule, NewAuditLog, NewDoor, NewDoorAccessEvent, NewDoorCheckin,
    UpdateDoor,
};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// Routers
// ---------------------------------------------------------------------------

pub fn member_routes() -> Router<AppState> {
    Router::new()
        .route("/{id}/info", get(door_info))
        .route("/{id}/checkin", post(door_checkin))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_doors).post(create_door))
        .route(
            "/{id}",
            get(get_door).patch(update_door).delete(delete_door),
        )
        .route("/{id}/unlock", post(admin_unlock))
        .route("/{id}/republish", post(admin_republish))
        .route("/{id}/qr", get(get_qr_url))
        .route("/{id}/events", get(list_events))
        .route("/{id}/rules", get(list_rules).post(add_rule))
        .route("/{id}/rules/{rule_id}", axum::routing::delete(remove_rule))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DoorSummary {
    pub id: Uuid,
    pub name: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub unlock_duration_ms: i32,
    pub enabled: bool,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub place_id_from: Option<Uuid>,
    pub place_id_to: Option<Uuid>,
}

impl From<Door> for DoorSummary {
    fn from(d: Door) -> Self {
        Self {
            id: d.id,
            name: d.name,
            location: d.location,
            description: d.description,
            unlock_duration_ms: d.unlock_duration_ms,
            enabled: d.enabled,
            created_at: d.created_at,
            updated_at: d.updated_at,
            place_id_from: d.place_id_from,
            place_id_to: d.place_id_to,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DoorDetail {
    #[serde(flatten)]
    pub door: DoorSummary,
    pub rules: Vec<AccessRule>,
}

#[derive(Debug, Deserialize)]
pub struct CreateDoorRequest {
    pub name: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub unlock_duration_ms: Option<i32>,
    pub enabled: Option<bool>,
    /// Required. Use a special place (e.g. `Outside`) for exterior doors.
    pub place_id_from: Uuid,
    /// Required.
    pub place_id_to: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDoorRequest {
    pub name: Option<String>,
    pub location: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub unlock_duration_ms: Option<i32>,
    pub enabled: Option<bool>,
    /// PATCH-style: `Some` = set; absent = leave alone. Doors can no longer
    /// be cleared to NULL — point at a special place if you mean "Outside".
    #[serde(default)]
    pub place_id_from: Option<Uuid>,
    #[serde(default)]
    pub place_id_to: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AddRuleRequest {
    pub kind: DoorRuleKind,
    pub value: String,
    #[serde(default = "default_effect")]
    pub effect: DoorRuleEffect,
    /// Optional weekly-window schedule that gates this rule. `None` = always.
    #[serde(default)]
    pub schedule_id: Option<Uuid>,
}
fn default_effect() -> DoorRuleEffect {
    DoorRuleEffect::Allow
}

#[derive(Debug, Deserialize, Default)]
pub struct EventsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DoorInfoResponse {
    pub id: Uuid,
    pub name: String,
    pub location: Option<String>,
    pub enabled: bool,
    pub you_are_authorized: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckinResponse {
    pub unlocked: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QrResponse {
    pub url: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().door.enabled {
        return Err(ApiError::Forbidden(
            "Door module is disabled in server configuration".to_string(),
        ));
    }
    Ok(())
}

/// Best-effort client IP for the checkin log.
///
/// #120 (#122/#13): X-Forwarded-For is a comma-separated chain
/// `client, proxy1, proxy2`; the *left*-most entry is set by the client and so
/// is forgeable (an attacker poisons the audit IP by sending any XFF they like).
/// The right-most entry is the one the nearest trusted reverse proxy appended,
/// so we take that. (This assumes the standard single-trusted-proxy deployment;
/// a directly-exposed server should prefer the peer socket address, and a
/// multi-proxy chain would strip a known number of trailing hops.)
fn client_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
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
        tracing::error!("Failed to write door audit log {}: {}", event.as_str(), e);
    }
}

fn maybe_publish_state(state: &AppState, door: &Door) {
    if !state.config_manager.get_config().door.enabled {
        return;
    }
    // #101: the coordinator is an `edge` binding rather than a column, so it is
    // looked up. A door with none is normal (nothing to publish to), which is why
    // that case is silent and only a lookup failure warns.
    match state.db.door_edge_device(door.id) {
        Ok(Some(device_id)) => {
            if let Err(e) = state.door_service.publish_state(device_id) {
                tracing::warn!("Failed to republish doors/state for {}: {}", device_id, e);
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("Failed to resolve the edge binding for {}: {}", door.id, e),
    }
}

// ---------------------------------------------------------------------------
// Member-facing handlers
// ---------------------------------------------------------------------------

/// GET /api/doors/{id}/info — for the QR landing page.
async fn door_info(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let door = state.db.get_door(id)?;
    let decision = state
        .door_service
        .evaluate(&door, &user.0)
        .map_err(ApiError::from)?;
    let (you_are_authorized, reason) = match decision {
        AccessDecision::Allow => (true, None),
        AccessDecision::Deny(reason) => (false, Some(reason)),
    };
    Ok(Json(ApiResponse::success(DoorInfoResponse {
        id: door.id,
        name: door.name,
        location: door.location,
        enabled: door.enabled,
        you_are_authorized,
        reason,
    })))
}

/// #120 (#122/#13): at most this many QR check-ins per member+door within the
/// window, so a static QR cannot be used to spam unlocks. A person legitimately
/// checks in once; this is generous enough not to bite real use.
const CHECKIN_MAX_ATTEMPTS: u32 = 5;
const CHECKIN_WINDOW_SECS: u32 = 30;

/// POST /api/doors/{id}/checkin — `I'm here, unlock the door`.
async fn door_checkin(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;

    // #120 (#122/#13): rate-limit before touching the DB or actuating anything.
    // Every check-in counts toward the window, so a flood is refused regardless
    // of whether each would be granted. (Proof-of-presence -- binding a check-in
    // to a rotating secret the door itself emits -- needs door-side display or
    // beacon hardware and is tracked separately; this bounds abuse meanwhile.)
    let throttle_key = format!("checkin:{}:{}", user.0.id, id);
    if let Err(retry_after) = state.throttle_service.check_attempt(
        &throttle_key,
        CHECKIN_MAX_ATTEMPTS,
        CHECKIN_WINDOW_SECS,
    ) {
        return Err(ApiError::TooManyRequests(format!(
            "Too many check-ins for this door; try again in {retry_after}s"
        )));
    }
    state.throttle_service.record_failed_attempt(
        &throttle_key,
        CHECKIN_MAX_ATTEMPTS,
        CHECKIN_WINDOW_SECS,
    );

    let door = state.db.get_door(id)?;

    let decision = state
        .door_service
        .evaluate(&door, &user.0)
        .map_err(ApiError::from)?;

    let ip = client_ip(&headers);
    let ua = user_agent(&headers);

    let (granted, reason) = match &decision {
        AccessDecision::Allow => (true, None),
        AccessDecision::Deny(r) => (false, Some(r.clone())),
    };

    // Record the access attempt regardless of outcome.
    let access_event = state.db.insert_door_access_event(&NewDoorAccessEvent {
        door_id: door.id,
        user_id: Some(user.0.id),
        method: DoorAccessMethod::QrCheckin.as_str().to_string(),
        card_id_attempted: None,
        granted,
        reason: reason.clone(),
        ip_address: ip.clone(),
        occurred_at: Utc::now(),
    })?;

    if granted {
        // Insert presence row.
        state.db.insert_door_checkin(&NewDoorCheckin {
            door_id: door.id,
            user_id: user.0.id,
            door_access_event_id: Some(access_event.id),
            ip_address: ip.clone(),
            user_agent: ua,
        })?;

        // Publish the unlock command if a coordinator is bound (#101: an `edge`
        // binding rather than a column).
        if let Some(device_id) = state.db.door_edge_device(door.id)? {
            if let Err(e) = state.door_service.publish_unlock(
                device_id,
                door.id,
                door.unlock_duration_ms,
                "qr_checkin",
            ) {
                tracing::warn!("Failed to publish doors/unlock: {}", e);
            }
        } else {
            tracing::warn!(
                "Door {} has no edge binding; unlock is logged only",
                door.id
            );
        }

        audit(
            &state,
            AuditEventType::DoorUnlockedQr,
            Some(user.0.id),
            serde_json::json!({
                "door_id": door.id,
                "door_name": door.name,
                "ip_address": ip,
            }),
        );
        audit(
            &state,
            AuditEventType::DoorCheckinRecorded,
            Some(user.0.id),
            serde_json::json!({ "door_id": door.id }),
        );

        Ok(Json(ApiResponse::success(CheckinResponse {
            unlocked: true,
            reason: None,
        })))
    } else {
        audit(
            &state,
            AuditEventType::DoorUnlockDenied,
            Some(user.0.id),
            serde_json::json!({
                "door_id": door.id,
                "door_name": door.name,
                "method": "qr_checkin",
                "reason": reason,
            }),
        );
        Ok(Json(ApiResponse::success(CheckinResponse {
            unlocked: false,
            reason,
        })))
    }
}

// ---------------------------------------------------------------------------
// Admin handlers
// ---------------------------------------------------------------------------

async fn list_doors(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let doors = state.db.list_doors()?;
    let resp: Vec<DoorSummary> = doors.into_iter().map(DoorSummary::from).collect();
    Ok(Json(ApiResponse::success(resp)))
}

async fn create_door(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateDoorRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".to_string()));
    }
    let cfg = state.config_manager.get_config();
    let new_door = NewDoor {
        name: req.name,
        location: req.location,
        description: req.description,
        unlock_duration_ms: req
            .unlock_duration_ms
            .unwrap_or(cfg.door.default_unlock_duration_ms),
        enabled: req.enabled.unwrap_or(true),
        created_by: Some(admin.0.id),
        place_id_from: req.place_id_from,
        place_id_to: req.place_id_to,
    };
    let door = state.db.create_door(&new_door)?;
    audit(
        &state,
        AuditEventType::DoorCreated,
        Some(admin.0.id),
        serde_json::json!({ "door_id": door.id, "name": door.name }),
    );
    maybe_publish_state(&state, &door);
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(DoorSummary::from(door))),
    ))
}

async fn get_door(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let door = state.db.get_door(id)?;
    let rules = state.db.list_rules_for_door(door.id)?;
    Ok(Json(ApiResponse::success(DoorDetail {
        door: DoorSummary::from(door),
        rules,
    })))
}

async fn update_door(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateDoorRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let changes = UpdateDoor {
        name: req.name,
        location: req.location,
        description: req.description,
        unlock_duration_ms: req.unlock_duration_ms,
        enabled: req.enabled,
        updated_at: Some(Utc::now()),
        place_id_from: req.place_id_from,
        place_id_to: req.place_id_to,
    };
    let door = state.db.update_door(id, &changes)?;
    audit(
        &state,
        AuditEventType::DoorUpdated,
        Some(admin.0.id),
        serde_json::json!({ "door_id": door.id }),
    );
    // #101: a door's coordinator can no longer change on this route -- it is an
    // `edge` binding now, and the bindings endpoint republishes to both the old
    // and the new device when it moves. So this republishes only to the door's
    // own coordinator.
    maybe_publish_state(&state, &door);
    Ok(Json(ApiResponse::success(DoorSummary::from(door))))
}

async fn delete_door(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let prev = state.db.get_door(id)?;
    // #101: resolve the coordinator BEFORE the delete. The binding row is cascaded
    // away with the resource, so afterwards there is nothing left to look up and
    // the old device would never be told the door is gone.
    let prev_device = state.db.door_edge_device(id)?;
    let deleted = state.db.delete_door(id)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Door not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::DoorDeleted,
        Some(admin.0.id),
        serde_json::json!({ "door_id": id, "name": prev.name }),
    );
    if let Some(device_id) = prev_device {
        if let Err(e) = state.door_service.publish_state(device_id) {
            tracing::warn!("Failed to republish after delete: {}", e);
        }
    }
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Door deleted".to_string()),
        error: None,
    }))
}

async fn admin_unlock(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let door = state.db.get_door(id)?;
    if !door.enabled {
        return Err(ApiError::BadRequest("Door is disabled".to_string()));
    }
    let device_id = state
        .db
        .door_edge_device(id)?
        .ok_or_else(|| ApiError::BadRequest("Door has no edge device bound".to_string()))?;
    state.door_service.publish_unlock(
        device_id,
        door.id,
        door.unlock_duration_ms,
        "admin_remote",
    )?;

    // Record an access event + audit so the action is visible.
    let _ = state.db.insert_door_access_event(&NewDoorAccessEvent {
        door_id: door.id,
        user_id: Some(admin.0.id),
        method: DoorAccessMethod::AdminRemote.as_str().to_string(),
        card_id_attempted: None,
        granted: true,
        reason: None,
        ip_address: None,
        occurred_at: Utc::now(),
    });
    audit(
        &state,
        AuditEventType::DoorUnlockedAdmin,
        Some(admin.0.id),
        serde_json::json!({ "door_id": door.id, "door_name": door.name }),
    );
    Ok(Json(ApiResponse::success(
        serde_json::json!({ "unlocked": true }),
    )))
}

async fn admin_republish(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Resolves the door to 404 a missing id before reporting a republish.
    let _door = state.db.get_door(id)?;
    if let Some(device_id) = state.db.door_edge_device(id)? {
        state.door_service.publish_state(device_id)?;
    }
    Ok(Json(ApiResponse::success(
        serde_json::json!({ "republished": true }),
    )))
}

async fn get_qr_url(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let _ = state.db.get_door(id)?; // ensure it exists
    let cfg = state.config_manager.get_config();
    let site_url = cfg.site.site_url.trim_end_matches('/');
    let url = cfg
        .door
        .qr_url_template
        .replace("{site_url}", site_url)
        .replace("{door_id}", &id.to_string());
    Ok(Json(ApiResponse::success(QrResponse { url })))
}

async fn list_events(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
    Query(q): Query<EventsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let events: Vec<DoorAccessEvent> = state.db.list_door_events(id, limit, offset)?;
    Ok(Json(ApiResponse::success(events)))
}

async fn list_rules(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let rules = state.db.list_rules_for_door(id)?;
    Ok(Json(ApiResponse::success(rules)))
}

async fn add_rule(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AddRuleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let door = state.db.get_door(id)?;
    // Open Access carries no `value` (the strike is held open for everyone while
    // its window is active), but it *requires* a schedule: an unscheduled
    // open-access rule would hold the door unlocked forever -- defeating access
    // control and leaving no window end to push to the edge as `hold_unlock_until`.
    let value = match req.kind {
        DoorRuleKind::OpenAccess => {
            if req.schedule_id.is_none() {
                return Err(ApiError::BadRequest(
                    "open_access rules require a schedule".to_string(),
                ));
            }
            String::new()
        }
        DoorRuleKind::Card => {
            // #120 (#122/H3): store the card's wire-digest, never the plaintext
            // code. Write-only, like a sealed card (#108): an admin sets a code
            // and the system keeps only its digest, so the rules table at rest
            // no longer holds an enumerable card store. Requires the card cipher.
            let raw = req.value.trim();
            if raw.is_empty() {
                return Err(ApiError::BadRequest("value is required".to_string()));
            }
            let cipher = state.card_cipher.as_deref().ok_or_else(|| {
                ApiError::Conflict(
                    "card rules require card encryption keys to be configured".to_string(),
                )
            })?;
            hex::encode(cipher.wire_digest(raw).map_err(|e| {
                ApiError::ValidationError(format!("could not process card value: {e}"))
            })?)
        }
        _ => {
            if req.value.trim().is_empty() {
                return Err(ApiError::BadRequest("value is required".to_string()));
            }
            req.value
        }
    };
    let rule = state.db.insert_door_rule(&NewAccessRule {
        resource_id: id,
        kind: req.kind.as_str().to_string(),
        value,
        effect: req.effect.as_str().to_string(),
        schedule_id: req.schedule_id,
    })?;
    audit(
        &state,
        AuditEventType::DoorRuleAdded,
        Some(admin.0.id),
        serde_json::json!({
            "door_id": id,
            "rule_id": rule.id,
            "kind": rule.kind,
            "value": rule.value,
            "effect": rule.effect,
        }),
    );
    maybe_publish_state(&state, &door);
    Ok((StatusCode::CREATED, Json(ApiResponse::success(rule))))
}

async fn remove_rule(
    State(state): State<AppState>,
    admin: AdminUser,
    Path((id, rule_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiError> {
    let door = state.db.get_door(id)?;
    let deleted = state.db.delete_door_rule(id, rule_id)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Rule not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::DoorRuleRemoved,
        Some(admin.0.id),
        serde_json::json!({ "door_id": id, "rule_id": rule_id }),
    );
    maybe_publish_state(&state, &door);
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Rule removed".to_string()),
        error: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::client_ip;
    use axum::http::HeaderMap;

    // #120 (#122/#13): the left-most X-Forwarded-For entry is client-set and
    // forgeable; the right-most is what the trusted proxy appended. Taking the
    // left-most would let an attacker poison the audit IP.
    #[test]
    fn client_ip_takes_the_rightmost_forwarded_for_not_the_client_value() {
        let mut h = HeaderMap::new();
        h.insert(
            "x-forwarded-for",
            "6.6.6.6, 10.0.0.2, 198.51.100.7".parse().unwrap(),
        );
        assert_eq!(client_ip(&h).as_deref(), Some("198.51.100.7"));
    }

    #[test]
    fn client_ip_falls_back_to_x_real_ip() {
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", "203.0.113.9".parse().unwrap());
        assert_eq!(client_ip(&h).as_deref(), Some("203.0.113.9"));
    }

    #[test]
    fn client_ip_is_none_without_headers() {
        assert_eq!(client_ip(&HeaderMap::new()), None);
    }
}
