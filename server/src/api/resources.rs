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
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::toolguard::{self, ToolGuardResponse, ToolLogRequest};
use crate::AppState;

pub fn resource_routes() -> Router<AppState> {
    Router::new()
        .route("/{id}/scan", post(scan))
        .route("/{id}/stop", post(stop))
        .route("/{id}/usage", post(usage))
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
        // A door has no session to end yet (its session is the pulse, over by
        // the time anyone could ask), so a stop on it is the late stop the
        // tool path already accepts: harmless, and recorded against nobody.
        Resource::Door(_) => ToolGuardResponse::ok_with_message("No session to end"),
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
