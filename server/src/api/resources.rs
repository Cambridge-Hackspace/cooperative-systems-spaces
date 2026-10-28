//! One path for a badge presented to any resource (#182).
//!
//! A door, a tool and a room are the same thing to a device: a resource a card
//! is presented to, whose bindings decide what follows a yes. Until #182 the
//! server offered a device two vocabularies for that -- `tool-on` / `tool-off`
//! / `tool-log` for one kind, and nothing at all for the other (a door's badge
//! path was decided on the edge from a compiled snapshot, and the only HTTP
//! route was the member's own QR check-in). These three routes are the one
//! vocabulary:
//!
//! - `POST /api/resources/{id}/scan`  `{ card }`             -- a badge
//! - `POST /api/resources/{id}/stop`  `{ card? }`            -- end the session
//! - `POST /api/resources/{id}/usage` `{ card?, seconds, … }` -- a usage report
//!
//! `{id}` is the resource's UUID or, for a tool, its short external id,
//! matched the way `tool_id` always was: external id first, then UUID. The
//! credential is the same as everywhere on the device surface -- a registered
//! device's Bearer token, bound to the resource in any role.
//!
//! The decision and what follows it are **by kind**, not by path: a tool's scan
//! runs the very function `tool-on` runs ([`toolguard::activate_tool`]), so the
//! alias and the resource path cannot drift; a door's scan runs the door's
//! policy through the same access engine the QR check-in does, records the same
//! access-event row the edge-reported badge does, and answers with the pulse
//! its `strike` binding says. The old names stay as aliases for one release.
//!
//! What the reply carries: `authorized`, and on a yes for a resource with a
//! `strike` binding, `strike: { pulse_ms }` -- the actuation. Nothing is pushed
//! to the door's coordinator for a scan answered here: the device that asked is
//! the device that acts, as it is for a lease.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::responses::ApiResponse;
use crate::api::toolguard::{self, ToolGuardResponse, ToolLogRequest};
use crate::auth::AdminUser;
use crate::models::{session_closed_by, session_profile, AuditEventType, NewAuditLog};
use crate::AppState;

pub fn resource_routes() -> Router<AppState> {
    Router::new()
        .route("/{id}/scan", post(scan))
        .route("/{id}/stop", post(stop))
        .route("/{id}/usage", post(usage))
        // #191: the policy for everything this device coordinates, one shape.
        .route("/policy", get(policy))
}

/// The resource-level attributes an administrator sets, whatever the kind
/// (#182). Mounted at `/api/admin/resources`.
pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/{id}", patch(update_resource))
        // #190: the open sessions on every resource, and ending one.
        .route("/sessions", get(list_open_sessions))
        .route("/{id}/stop", post(admin_stop))
}

// ---------------------------------------------------------------------------
// Administration: what is true of a resource regardless of its kind
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct UpdateResourceRequest {
    /// `momentary` | `until_stopped`; see `models::resources::session_profile`.
    #[serde(default)]
    pub session_profile: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ResourceSummary {
    pub id: Uuid,
    pub kind: String,
    pub session_profile: String,
}

/// PATCH /api/admin/resources/{id}
///
/// The session profile is the first resource-level attribute, and the one the
/// coordinator reads: a door set to `until_stopped` keeps its session -- and
/// its power bindings leased -- until somebody stops it, which is the room.
async fn update_resource(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateResourceRequest>,
) -> Result<Json<ApiResponse<ResourceSummary>>, ApiError> {
    let Some(profile) = req.session_profile.as_deref() else {
        return Err(ApiError::BadRequest(
            "nothing to change: send session_profile".to_string(),
        ));
    };
    let profile =
        crate::api::tool_modules::one_of(profile, &session_profile::ALL, "session_profile")?;
    let Some(updated) = state.db.update_resource_session_profile(id, &profile)? else {
        return Err(ApiError::NotFound("Resource not found".to_string()));
    };

    let log = NewAuditLog {
        event_type: AuditEventType::ResourceUpdated.as_str().to_string(),
        user_id: Some(admin.0.id),
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "resource_id": updated.id,
            "kind": updated.kind,
            "session_profile": updated.session_profile,
        }),
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write resource_updated audit log: {}", e);
    }
    // The profile travels in the wiring snapshot, and the coordinator acts on
    // it, so the change reaches it now rather than at the next poll.
    toolguard::broadcast_module_state(&state).await;

    Ok(Json(ApiResponse::success(ResourceSummary {
        id: updated.id,
        kind: updated.kind,
        session_profile: updated.session_profile,
    })))
}

// ---------------------------------------------------------------------------
// Bodies
// ---------------------------------------------------------------------------
//
// Handlers take `Bytes` and parse after authenticating, for the reason
// `toolguard::ToolRequestWire` gives: an extractor that answers 415 or 422
// before the handler runs tells an unauthenticated caller what the body looks
// like.

#[derive(Debug, Deserialize)]
struct ScanBody {
    card: String,
}

#[derive(Debug, Deserialize)]
struct StopBody {
    #[serde(default)]
    card: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageBody {
    #[serde(default)]
    card: Option<String>,
    seconds: f32,
    #[serde(default)]
    active_seconds: Option<f32>,
    #[serde(default)]
    temperature: Option<f32>,
}

fn parse<T: for<'de> Deserialize<'de>>(body: &Bytes, shape: &str) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|_| ApiError::BadRequest(format!("body must be a JSON object: {shape}")))
}

// ---------------------------------------------------------------------------
// The resource, and who may speak for it
// ---------------------------------------------------------------------------

/// A resource as a device names it, resolved to its kind.
pub(crate) enum Resource {
    Tool(crate::models::Tool),
    Door(crate::models::Door),
}

/// Resolve `{id}`: a tool by external id or UUID (the order `tool_id` has
/// always been matched in), else a door by UUID.
pub(crate) async fn find_resource(
    state: &AppState,
    id: &str,
) -> Result<Option<Resource>, ApiError> {
    if let Some(tool) = toolguard::find_tool_by_toolguard_id(state, id).await? {
        return Ok(Some(Resource::Tool(tool)));
    }
    if let Ok(uuid) = Uuid::parse_str(id) {
        if let Some(kind) = state.db.resource_kind(uuid)? {
            if kind == crate::models::ResourceKind::Door.as_str() {
                return Ok(Some(Resource::Door(state.db.get_door(uuid)?)));
            }
        }
    }
    Ok(None)
}

/// Authenticate the caller as a registered device bound to the named resource
/// in any role, and hand back the resource.
///
/// The same rule as `authorize_toolguard`, resource-shaped: the credential is
/// read first (an `InternalServerError` from that is re-raised unchanged -- a
/// database fault must not read as a credential problem), then the resource is
/// resolved and the binding checked. An unknown resource and an unbound one
/// get the same answer, so an authenticated but unbound device learns nothing
/// about what exists.
pub(crate) async fn authorize_resource(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
) -> Result<(Uuid, Resource), ApiError> {
    let (device_id, _token) = match toolguard::extract_device_auth(state, headers).await {
        Ok(v) => v,
        Err(ApiError::InternalServerError(e)) => return Err(ApiError::InternalServerError(e)),
        Err(_) => {
            return Err(ApiError::Unauthorized(
                "Resource operations require a registered device token".to_string(),
            ))
        }
    };
    let Some(resource) = find_resource(state, id).await? else {
        return Err(ApiError::Unauthorized(
            "This device is not bound to that resource".to_string(),
        ));
    };
    let resource_id = match &resource {
        Resource::Tool(t) => t.id,
        Resource::Door(d) => d.id,
    };
    // A tool's id is its resource id (#101), so the binding primitive is
    // already resource-shaped.
    if state.db.device_is_bound_to_tool(device_id, resource_id)? {
        return Ok((device_id, resource));
    }
    Err(ApiError::Unauthorized(
        "This device is not bound to that resource".to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/resources/{id}/scan
async fn scan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let (device_id, resource) = authorize_resource(&state, &headers, &id).await?;
    let req: ScanBody = parse(&body, "{ card }")?;
    // The card is deliberately absent from this line (#107).
    tracing::info!("Scan: resource={}", id);
    let reply = match resource {
        Resource::Tool(tool) => toolguard::activate_tool(&state, Some(tool), &req.card, &id)
            .await?
            .as_resource_reply(),
        Resource::Door(door) => scan_door(&state, device_id, &door, &req.card).await?,
    };
    Ok(Json(reply))
}

/// POST /api/resources/{id}/stop
async fn stop(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let (_device_id, resource) = authorize_resource(&state, &headers, &id).await?;
    let req: StopBody = parse(&body, "{ card? }")?;
    tracing::info!("Stop: resource={}", id);
    let reply = match resource {
        Resource::Tool(tool) => {
            toolguard::deactivate_tool(&state, Some(tool), req.card.as_deref(), &id)
                .await?
                .as_resource_reply()
        }
        // #190: a door's `until_stopped` session ends here, as a tool's does;
        // a momentary door never had one past its pulse, and a stop on it is
        // the late stop the tool path already accepts: harmless, and recorded
        // against nobody.
        Resource::Door(door) => {
            match state
                .db
                .close_resource_session(door.id, session_closed_by::STOP)?
            {
                Some(_) => {
                    toolguard::broadcast_module_state(&state).await;
                    ToolGuardResponse::ok_with_message("Session ended")
                }
                None => ToolGuardResponse::ok_with_message("No session to end"),
            }
        }
    };
    Ok(Json(reply))
}

/// POST /api/resources/{id}/usage
async fn usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let (_device_id, resource) = authorize_resource(&state, &headers, &id).await?;
    let req: UsageBody = parse(&body, "{ card?, seconds, active_seconds?, temperature? }")?;
    tracing::info!("Usage: resource={} seconds={}", id, req.seconds);
    let reply = match resource {
        Resource::Tool(tool) => {
            let log = ToolLogRequest {
                card: req.card,
                tool_id: id.clone(),
                seconds: req.seconds,
                active_seconds: req.active_seconds,
                temperature: req.temperature,
            };
            toolguard::report_tool_usage(&state, Some(tool), &log).await?
        }
        // No session, so nothing for the time to belong to: the same refusal a
        // report on an idle tool gets.
        Resource::Door(_) => ToolGuardResponse::error("No open session for this resource"),
    };
    Ok(Json(reply))
}

// ---------------------------------------------------------------------------
// A badge at a door
// ---------------------------------------------------------------------------

/// A card presented to a door by a device: the door's own policy through the
/// one access engine, the same access-event row a badge reported by the edge
/// gets (method `rfid`), the same audit events, and on a yes the pulse from the
/// door's strike binding.
async fn scan_door(
    state: &AppState,
    device_id: Uuid,
    door: &crate::models::Door,
    card: &str,
) -> Result<ToolGuardResponse, ApiError> {
    use crate::models::{
        AuditEventType, CardResolution, DoorAccessMethod, NewAuditLog, NewDoorAccessEvent,
    };

    if !state.config_manager.get_config().door.enabled {
        return Ok(ToolGuardResponse::resource_denied(
            "Door module is disabled in server configuration",
        ));
    }

    // The card, resolved status-aware exactly as the tool path and the
    // edge-reported badge resolve it: an active card names its member; a
    // disabled or released one still names its member for the record and
    // raises the distinct fraud signal; an unknown code names nobody and we
    // keep no copy of its value (#107, #33).
    let mut card_id: Option<Uuid> = None;
    let mut revoked: Option<crate::models::UserCard> = None;
    let user = match toolguard::resolve_card(state, card).await? {
        CardResolution::Active { user, card } => {
            card_id = card.as_ref().map(|c| c.id);
            if let Some(c) = card {
                let _ = state.db.touch_card_last_used(c.id);
            }
            Some(user)
        }
        CardResolution::Revoked { user, card } => {
            card_id = Some(card.id);
            revoked = Some(card);
            Some(user)
        }
        CardResolution::Unknown => None,
    };

    let (granted, reason): (bool, Option<String>) = match (&user, &revoked) {
        (None, _) => (false, Some("Unknown card".to_string())),
        (Some(_), Some(_)) => (false, Some("Card not authorized".to_string())),
        (Some(user), None) => {
            match state
                .door_service
                .evaluate(door, user, crate::access_engine::Action::Unlock)?
            {
                crate::doors::AccessDecision::Allow => (true, None),
                crate::doors::AccessDecision::Deny(why) => (false, Some(why)),
            }
        }
    };

    // The access event, granted or not: a door's record is every presentation.
    state.db.insert_door_access_event(&NewDoorAccessEvent {
        door_id: door.id,
        user_id: user.as_ref().map(|u| u.id),
        method: DoorAccessMethod::Rfid.as_str().to_string(),
        // The resolved card's UUID, never the code (#120, #123/H5).
        card_id_attempted: card_id.map(|c| c.to_string()),
        granted,
        reason: reason.clone(),
        ip_address: None,
        occurred_at: chrono::Utc::now(),
    })?;

    let user_id = user.as_ref().map(|u| u.id);
    if let (Some(uid), Some(card)) = (user_id, revoked.as_ref()) {
        let _ = state.db.create_audit_log(&NewAuditLog {
            event_type: AuditEventType::RevokedCardPresented.as_str().to_string(),
            user_id: Some(uid),
            actor_id: Some(uid),
            event_data: serde_json::json!({
                "door_id": door.id,
                "card_id": card.id,
                "card_status": card.status,
                "context": "door",
            }),
            ip_address: None,
            user_agent: None,
        });
    }
    let audit_type = if granted {
        AuditEventType::DoorUnlockedCard
    } else {
        AuditEventType::DoorUnlockDenied
    };
    let _ = state.db.create_audit_log(&NewAuditLog {
        event_type: audit_type.as_str().to_string(),
        user_id,
        actor_id: user_id,
        event_data: serde_json::json!({
            "door_id": door.id,
            "door_name": door.name,
            "device_id": device_id,
            "card_id": card_id,
            "granted": granted,
            "reason": reason,
            "source": DoorAccessMethod::Rfid.as_str(),
            "path": "resource_scan",
        }),
        ip_address: None,
        user_agent: None,
    });

    if !granted {
        return Ok(ToolGuardResponse::resource_denied(
            reason.unwrap_or_else(|| "Denied".to_string()),
        ));
    }
    // #190: a yes on an `until_stopped` door opens a session the platform
    // keeps -- what a tool's yes has always done through `tools.status`. A
    // momentary door's session is the pulse, over by the time anyone could
    // ask; nothing is recorded for it beyond the access event above. A second
    // badge at a room in use joins the open session rather than failing.
    let profile = state
        .db
        .get_resource(door.id)?
        .map(|r| r.session_profile)
        .unwrap_or_default();
    if profile == session_profile::UNTIL_STOPPED {
        state
            .db
            .open_resource_session(door.id, user_id, Some(device_id))?;
        toolguard::broadcast_module_state(state).await;
    }
    // The pulse is the strike binding's profile, or the site default for a
    // door nobody gave a strike (#182). A door is never without one: a reader
    // that also holds the latch needs a number, and the default is what the
    // door opened for before the latch was a binding.
    let strike = state.door_service.strike_profile(door.id)?;
    Ok(ToolGuardResponse::resource_authorized(Some(
        css_lib::wire::LocalStrike {
            pulse_ms: strike.pulse_ms,
        },
    )))
}

// ---------------------------------------------------------------------------
// Sessions, whatever the kind (#190)
// ---------------------------------------------------------------------------

/// One open session as the platform shows it, whatever the resource's kind
/// (#190): a door's from `resource_sessions`, a tool's from `tools.status` and
/// its activation event, which is where a tool's session still lives.
#[derive(Debug, Serialize)]
pub struct OpenSessionView {
    pub resource_id: Uuid,
    pub kind: String,
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
    pub full_name: Option<String>,
    pub opened_at: chrono::DateTime<chrono::Utc>,
    pub device_id: Option<Uuid>,
}

/// GET /api/admin/resources/sessions -- every resource in use, with who.
async fn list_open_sessions(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<ApiResponse<Vec<OpenSessionView>>>, ApiError> {
    let mut views: Vec<OpenSessionView> = Vec::new();
    for s in state.db.open_resource_sessions()? {
        views.push(OpenSessionView {
            resource_id: s.resource_id,
            kind: crate::models::ResourceKind::Door.as_str().to_string(),
            user_id: s.user_id,
            username: None,
            full_name: None,
            opened_at: s.opened_at,
            device_id: s.device_id,
        });
    }
    for tool in state.db.get_inuse_tools()? {
        let activation = state.db.latest_activation(tool.id)?;
        views.push(OpenSessionView {
            resource_id: tool.id,
            kind: crate::models::ResourceKind::Tool.as_str().to_string(),
            user_id: activation.as_ref().and_then(|e| e.user_id),
            username: None,
            full_name: None,
            opened_at: activation
                .as_ref()
                .map_or(tool.updated_at, |e| e.created_at),
            device_id: None,
        });
    }
    let ids: Vec<Uuid> = views.iter().filter_map(|v| v.user_id).collect();
    let names: std::collections::HashMap<Uuid, (String, String)> = state
        .db
        .user_names(&ids)?
        .into_iter()
        .map(|(id, username, full_name)| (id, (username, full_name)))
        .collect();
    for v in views.iter_mut() {
        if let Some((username, full_name)) = v.user_id.and_then(|id| names.get(&id)) {
            v.username = Some(username.clone());
            v.full_name = Some(full_name.clone());
        }
    }
    views.sort_by(|a, b| a.opened_at.cmp(&b.opened_at));
    Ok(Json(ApiResponse::success(views)))
}

#[derive(Debug, Serialize)]
pub struct SessionEnded {
    pub resource_id: Uuid,
    pub kind: String,
    /// False when nothing was open: a late stop is harmless, and says so.
    pub ended: bool,
}

/// POST /api/admin/resources/{id}/stop -- end a resource's open session from
/// the platform, whatever its kind (#190). A tool's is ended the way a device's
/// card-less stop ends it; a door's by closing the record the badge opened.
async fn admin_stop(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> Result<Json<ApiResponse<SessionEnded>>, ApiError> {
    let Some(resource) = find_resource(&state, &id).await? else {
        return Err(ApiError::NotFound("Resource not found".to_string()));
    };
    let (resource_id, kind, ended) = match resource {
        Resource::Tool(tool) => {
            let was_in_use = tool.status == crate::models::ToolStatus::InUse;
            let tool_id = tool.id;
            toolguard::deactivate_tool(&state, Some(tool), None, &id).await?;
            (tool_id, "tool", was_in_use)
        }
        Resource::Door(door) => {
            let closed = state
                .db
                .close_resource_session(door.id, session_closed_by::ADMIN)?;
            if closed.is_some() {
                toolguard::broadcast_module_state(&state).await;
            }
            (door.id, "door", closed.is_some())
        }
    };
    let log = NewAuditLog {
        event_type: AuditEventType::ResourceSessionEnded.as_str().to_string(),
        user_id: Some(admin.0.id),
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "resource_id": resource_id,
            "kind": kind,
            "ended": ended,
        }),
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write resource_session_ended audit log: {}", e);
    }
    Ok(Json(ApiResponse::success(SessionEnded {
        resource_id,
        kind: kind.to_string(),
        ended,
    })))
}

// ---------------------------------------------------------------------------
// The policy, one shape whatever the kind (#191)
// ---------------------------------------------------------------------------

/// Build one device's resource policy: the tools it is bound to and their
/// principals (the allow-list, as `toolguard/sync` has always built it), the
/// doors it coordinates and their compiled card lists (as `doors/state` has),
/// and every resource's kind, profile, pulse and whether it is in use.
pub async fn build_resource_policy(
    state: &AppState,
    device_id: Uuid,
) -> Result<css_lib::wire::ResourcePolicyPayload, ApiError> {
    let config = state.config_manager.get_config();
    let profile_field = config.toolguard.profile_field.clone();
    let tools = toolguard::build_sync_payload(state, device_id, &profile_field).await?;
    let doors = state.door_service.compile_state_for(device_id)?;
    let profiles: std::collections::HashMap<Uuid, (String, String)> = state
        .db
        .resource_profiles()?
        .into_iter()
        .map(|(id, kind, profile)| (id, (kind, profile)))
        .collect();
    let in_use: std::collections::HashSet<Uuid> = state
        .db
        .get_inuse_tools()?
        .into_iter()
        .map(|t| t.id)
        .chain(
            state
                .db
                .open_resource_sessions()?
                .into_iter()
                .map(|s| s.resource_id),
        )
        .collect();

    let mut resources = Vec::with_capacity(tools.tools.len() + doors.doors.len());
    for t in &tools.tools {
        let (kind, profile) = profiles.get(&t.id).cloned().unwrap_or_else(|| {
            (
                crate::models::ResourceKind::Tool.as_str().to_string(),
                String::new(),
            )
        });
        resources.push(css_lib::wire::ResourcePolicy {
            id: t.id.to_string(),
            external_id: t.external_id.clone(),
            name: t.name.clone(),
            kind,
            session_profile: profile,
            status: serde_json::to_value(&t.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default(),
            requires_online: t.requires_online,
            in_use: in_use.contains(&t.id),
            enabled: true,
            pulse_ms: state.db.door_strike(t.id)?.map(|s| s.pulse_ms),
            allow: Vec::new(),
            deny: Vec::new(),
            hold_until: None,
        });
    }
    for d in &doors.doors {
        let profile = profiles
            .get(&d.id)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        resources.push(css_lib::wire::ResourcePolicy {
            id: d.id.to_string(),
            external_id: None,
            name: d.name.clone(),
            kind: crate::models::ResourceKind::Door.as_str().to_string(),
            session_profile: profile,
            status: String::new(),
            requires_online: false,
            in_use: in_use.contains(&d.id),
            enabled: d.enabled,
            pulse_ms: Some(d.unlock_duration_ms),
            allow: d.allow_cards.clone(),
            deny: d.deny_cards.clone(),
            hold_until: d.hold_unlock_until.map(|t| t.to_rfc3339()),
        });
    }
    let principals = tools
        .users
        .iter()
        .map(|u| css_lib::wire::Principal {
            digest: u.profile_field_digest.clone(),
            full_name: u.full_name.clone(),
            is_active: u.is_active,
            authorized: u
                .authorized_tool_ids
                .iter()
                .map(|id| id.to_string())
                .collect(),
        })
        .collect();
    Ok(css_lib::wire::ResourcePolicyPayload {
        device_id: device_id.to_string(),
        as_of: chrono::Utc::now().to_rfc3339(),
        profile_field,
        resources,
        principals,
    })
}

/// GET /api/resources/policy -- the policy for everything this device
/// coordinates (#191). Device Bearer token; the push twin is `resource/policy`.
async fn policy(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<css_lib::wire::ResourcePolicyPayload>, ApiError> {
    let (device_id, _) = toolguard::extract_device_auth(&state, &headers).await?;
    Ok(Json(build_resource_policy(&state, device_id).await?))
}
