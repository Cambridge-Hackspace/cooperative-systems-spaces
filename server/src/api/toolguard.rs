use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::AppState;

/// Standard ToolGuard API response
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolGuardResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_off: Option<bool>,
}

impl ToolGuardResponse {
    fn ok() -> Self {
        Self { status: "ok".to_string(), message: None, tool_on: None, tool_off: None }
    }

    fn ok_with_message(message: impl Into<String>) -> Self {
        Self { status: "ok".to_string(), message: Some(message.into()), tool_on: None, tool_off: None }
    }

    fn error(message: impl Into<String>) -> Self {
        Self { status: "error".to_string(), message: Some(message.into()), tool_on: None, tool_off: None }
    }

    fn tool_authorized() -> Self {
        Self {
            status: "ok".to_string(),
            message: Some("Tool authorized".to_string()),
            tool_on: Some(true),
            tool_off: None,
        }
    }

    fn tool_denied(reason: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            message: Some(reason.into()),
            tool_on: Some(false),
            tool_off: None,
        }
    }

    fn tool_off_ok() -> Self {
        Self {
            status: "ok".to_string(),
            message: Some("Tool deactivated".to_string()),
            tool_on: None,
            tool_off: Some(true),
        }
    }
}

/// Request parameters for tool operations
#[derive(Debug, Deserialize)]
pub struct ToolRequest {
    pub card: String,
    pub tool_id: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Request parameters for tool logging
#[derive(Debug, Deserialize)]
pub struct ToolLogRequest {
    pub card: String,
    pub tool_id: String,
    pub seconds: f32,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub api_key: Option<String>,
}

// ── Sync payload types (shared between HTTP response and MQTT publish) ──────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGuardSyncTool {
    pub id: Uuid,
    pub external_id: Option<String>,
    pub name: String,
    pub status: crate::models::ToolStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGuardSyncUser {
    pub profile_field_value: String,
    pub full_name: String,
    pub is_active: bool,
    pub authorized_tool_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolGuardSyncPayload {
    pub device_id: Uuid,
    pub profile_field: String,
    pub tools: Vec<ToolGuardSyncTool>,
    pub users: Vec<ToolGuardSyncUser>,
}

/// Configure ToolGuard API routes
pub fn toolguard_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(api_status))
        .route("/tool-on", get(tool_on))
        .route("/tool-off", get(tool_off))
        .route("/tool-log", get(tool_log))
        .route("/sync", get(sync))
        .route("/boot-reset", post(boot_reset))
}

/// GET /api/toolguard - API status check
async fn api_status() -> Json<ToolGuardResponse> {
    Json(ToolGuardResponse::ok())
}

/// GET /api/toolguard/sync - Return toolguard state for this device
/// Authenticated with device Bearer token
async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ToolGuardSyncPayload>, ApiError> {
    let (device_id, _) = extract_device_auth(&state, &headers).await?;

    let config = state.config_manager.get_config();
    let profile_field = config.toolguard.profile_field.clone();

    let payload = build_sync_payload(&state, device_id, &profile_field).await?;
    Ok(Json(payload))
}

/// POST /api/toolguard/boot-reset - Reset all InUse tools to Idle at edge boot
/// Authenticated with device Bearer token (no card required)
async fn boot_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let (device_id, _) = extract_device_auth(&state, &headers).await?;

    tracing::info!("Boot-reset requested by device {}", device_id);

    let inuse_tools = state.db.get_inuse_tools()
        .map_err(|e| ApiError::InternalServerError(format!("Failed to query InUse tools: {}", e)))?;

    let count = inuse_tools.len();
    for tool in inuse_tools {
        state.db.update_tool_status(tool.id, &crate::models::ToolStatus::Idle)
            .map_err(|e| ApiError::InternalServerError(format!("Failed to reset tool {}: {}", tool.id, e)))?;

        use crate::models::NewToolEvent;
        let event = NewToolEvent {
            tool_id: tool.id,
            event_type: "deactivated".to_string(),
            old_status: Some(tool.status.clone()),
            new_status: Some(crate::models::ToolStatus::Idle),
            user_id: None,
            actor_id: None,
            notes: Some(format!("Reset to idle at edge boot (device {})", device_id)),
            scan_data: Some(serde_json::json!({ "device_id": device_id, "reason": "boot-reset" })),
        };
        state.db.create_tool_event(&event)
            .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;
    }

    if count > 0 {
        tracing::info!("Boot-reset: {} tool(s) reset to idle by device {}", count, device_id);

        let audit_logger = state.audit_logger.clone();
        let details = serde_json::json!({
            "device_id": device_id,
            "tools_reset": count,
            "reason": "boot-reset",
        });
        tokio::spawn(async move {
            let _ = audit_logger.log_event(
                crate::models::AuditEventType::ToolDeactivated,
                None, None, details, None, None,
            ).await;
        });

        broadcast_toolguard_state(&state).await;
    }

    Ok(Json(ToolGuardResponse::ok_with_message(format!("{} tool(s) reset to idle", count))))
}

/// GET /api/toolguard/tool-on
async fn tool_on(
    State(state): State<AppState>,
    Query(req): Query<ToolRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    tracing::info!("Tool on request: card={}, tool_id={}", req.card, req.tool_id);

    let user = match find_user_by_card(&state, &req.card).await? {
        Some(u) => u,
        None => {
            log_tool_access_denied(&state, None, &req.tool_id, "Unknown card").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Unknown card")));
        }
    };

    if !user.is_active {
        log_tool_access_denied(&state, Some(&user), &req.tool_id, "User is not active").await?;
        return Ok(Json(ToolGuardResponse::tool_denied("User is not active")));
    }

    let tool = match find_tool_by_toolguard_id(&state, &req.tool_id).await? {
        Some(t) => t,
        None => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool not found").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool not found")));
        }
    };

    match tool.status {
        crate::models::ToolStatus::Idle => {}
        crate::models::ToolStatus::InUse => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is already in use").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool is already in use")));
        }
        crate::models::ToolStatus::Maintenance => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is under maintenance").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool is under maintenance")));
        }
        crate::models::ToolStatus::Broken => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is broken").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool is broken")));
        }
        crate::models::ToolStatus::Repair => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is in repair").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool is in repair")));
        }
        crate::models::ToolStatus::Retired => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is retired").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool is retired")));
        }
    }

    let has_training_steps = state.db.tool_has_training_steps(tool.id)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to check training steps: {}", e)))?;

    if has_training_steps {
        let has_completed = state.db.user_has_completed_all_training_steps(user.id, tool.id)
            .map_err(|e| ApiError::InternalServerError(format!("Failed to check training completion: {}", e)))?;
        if !has_completed {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Training required").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Training required")));
        }
    }

    state.db.update_tool_status(tool.id, &crate::models::ToolStatus::InUse)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to update tool status: {}", e)))?;

    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "activated".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: Some(crate::models::ToolStatus::InUse),
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!("Activated via ToolGuard (tool_id: {})", req.tool_id)),
        scan_data: Some(serde_json::json!({
            "toolguard_id": req.tool_id,
            "card": req.card,
        })),
    };
    state.db.create_tool_event(&event)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;

    log_tool_activated(&state, &user, tool.id, &req.tool_id).await?;

    // Broadcast updated state over MQTT
    broadcast_toolguard_state(&state).await;

    Ok(Json(ToolGuardResponse::tool_authorized()))
}

/// GET /api/toolguard/tool-off
async fn tool_off(
    State(state): State<AppState>,
    Query(req): Query<ToolRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    tracing::info!("Tool off request: card={}, tool_id={}", req.card, req.tool_id);

    let user = match find_user_by_card(&state, &req.card).await? {
        Some(u) => u,
        None => return Ok(Json(ToolGuardResponse::error("Unknown card"))),
    };

    let tool = match find_tool_by_toolguard_id(&state, &req.tool_id).await? {
        Some(t) => t,
        None => return Ok(Json(ToolGuardResponse::error("Tool not found"))),
    };

    state.db.update_tool_status(tool.id, &crate::models::ToolStatus::Idle)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to update tool status: {}", e)))?;

    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "deactivated".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: Some(crate::models::ToolStatus::Idle),
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!("Deactivated via ToolGuard (tool_id: {})", req.tool_id)),
        scan_data: Some(serde_json::json!({
            "toolguard_id": req.tool_id,
            "card": req.card,
        })),
    };
    state.db.create_tool_event(&event)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;

    log_tool_deactivated(&state, &user, tool.id, &req.tool_id).await?;

    broadcast_toolguard_state(&state).await;

    Ok(Json(ToolGuardResponse::tool_off_ok()))
}

/// GET /api/toolguard/tool-log
async fn tool_log(
    State(state): State<AppState>,
    Query(req): Query<ToolLogRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    tracing::info!(
        "Tool log request: card={}, tool_id={}, seconds={}, temp={:?}",
        req.card, req.tool_id, req.seconds, req.temperature
    );

    let user = match find_user_by_card(&state, &req.card).await? {
        Some(u) => u,
        None => return Ok(Json(ToolGuardResponse::error("Unknown card"))),
    };

    let tool = match find_tool_by_toolguard_id(&state, &req.tool_id).await? {
        Some(t) => t,
        None => return Ok(Json(ToolGuardResponse::error("Tool not found"))),
    };

    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "usage_logged".to_string(),
        old_status: None,
        new_status: None,
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!("Usage logged: {:.1} minutes", req.seconds / 60.0)),
        scan_data: Some(serde_json::json!({
            "toolguard_id": req.tool_id,
            "card": req.card,
            "seconds": req.seconds,
            "temperature": req.temperature,
        })),
    };
    state.db.create_tool_event(&event)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to create tool event: {}", e)))?;

    log_tool_usage(&state, &user, tool.id, &req.tool_id, req.seconds, req.temperature).await?;

    Ok(Json(ToolGuardResponse::ok_with_message("Usage logged")))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract and validate a device Bearer token from Authorization header.
/// Returns (device_id, auth_token) on success.
pub async fn extract_device_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Uuid, String), ApiError> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::Unauthorized("Missing Authorization header".to_string()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::Unauthorized("Invalid Authorization format".to_string()))?;

    let (device_id, _) = state.db.find_device_by_auth_token(token)
        .map_err(|e| ApiError::InternalServerError(format!("DB error: {}", e)))?
        .ok_or_else(|| ApiError::Unauthorized("Invalid device token".to_string()))?;

    Ok((device_id, token.to_string()))
}

/// Build the sync payload for a given device.
pub async fn build_sync_payload(
    state: &AppState,
    device_id: Uuid,
    profile_field: &str,
) -> Result<ToolGuardSyncPayload, ApiError> {
    let (mut users, mut tools) = state.db.get_toolguard_sync_data(profile_field)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to build sync data: {}", e)))?;

    // Apply schedule gating: any tool whose attached schedule is closed
    // right now is removed from every user's authorized list, and then
    // dropped from the top-level tool list since nobody can use it.
    let closed_tool_ids = closed_tool_ids_now(state)?;
    if !closed_tool_ids.is_empty() {
        for u in users.iter_mut() {
            u.authorized_tool_ids.retain(|tid| !closed_tool_ids.contains(tid));
        }
        tools.retain(|t| !closed_tool_ids.contains(&t.id));
    }

    Ok(ToolGuardSyncPayload {
        device_id,
        profile_field: profile_field.to_string(),
        tools,
        users,
    })
}

/// Tools whose attached schedule isn't currently open. Empty when no tools
/// have a schedule (typical case) or when no schedules exist.
fn closed_tool_ids_now(
    state: &AppState,
) -> Result<std::collections::HashSet<Uuid>, ApiError> {
    use std::collections::HashSet;
    let cfg = state.config_manager.get_config();
    let tz = crate::schedules::resolve_tz(&cfg.site.timezone);

    let schedules = state
        .db
        .list_schedules()
        .map_err(|e| ApiError::InternalServerError(format!("list_schedules: {e}")))?;
    if schedules.is_empty() {
        return Ok(HashSet::new());
    }

    // Index schedules by id for O(1) lookup.
    let by_id: std::collections::HashMap<Uuid, &crate::models::Schedule> =
        schedules.iter().map(|s| (s.id, s)).collect();

    // Walk every tool's `schedule_id`; cheaper than per-tool lookups.
    use crate::schema::tools::dsl;
    use diesel::prelude::*;
    let mut conn = state
        .db
        .pool()
        .get()
        .map_err(|e| ApiError::InternalServerError(format!("DB pool: {e}")))?;
    let rows: Vec<(Uuid, Option<Uuid>)> = dsl::tools
        .select((dsl::id, dsl::schedule_id))
        .load(&mut conn)
        .map_err(|e| ApiError::InternalServerError(format!("load tools: {e}")))?;

    let mut closed = HashSet::new();
    for (tool_id, schedule_id) in rows {
        let sid = match schedule_id {
            Some(s) => s,
            None => continue, // No schedule = always open.
        };
        let sched = match by_id.get(&sid) {
            Some(s) => s,
            None => continue, // Schedule went missing; treat as always open.
        };
        let intervals = match crate::schedules::parse_intervals(&sched.intervals) {
            Ok(v) => v,
            Err(_) => continue, // Invalid intervals — fail-open rather than locking the tool.
        };
        if !crate::schedules::matches_now(&intervals, tz) {
            closed.insert(tool_id);
        }
    }
    Ok(closed)
}

/// Broadcast the current toolguard state to all registered devices over MQTT.
pub async fn broadcast_toolguard_state(state: &AppState) {
    let mqtt_service = match &state.mqtt_service {
        Some(s) => s.clone(),
        None => return,
    };

    let config = state.config_manager.get_config();
    let profile_field = config.toolguard.profile_field.clone();

    let devices = match state.db.list_approved_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to list devices for toolguard broadcast: {}", e);
            return;
        }
    };

    for device_id in devices {
        match build_sync_payload(state, device_id, &profile_field).await {
            Ok(payload) => {
                match serde_json::to_vec(&payload) {
                    Ok(bytes) => {
                        if let Err(e) = mqtt_service.publish_toolguard_state(device_id, bytes) {
                            tracing::warn!("Failed to publish toolguard state to device {}: {}", device_id, e);
                        }
                    }
                    Err(e) => tracing::warn!("Failed to serialize toolguard payload: {}", e),
                }
            }
            Err(e) => tracing::warn!("Failed to build sync payload for device {}: {}", device_id, e),
        }
    }
}

async fn validate_api_key(state: &AppState, api_key: &str, tool: Option<&crate::models::Tool>) -> Result<bool, ApiError> {
    let config = state.config_manager.get_config();
    if api_key.is_empty() {
        return Ok(false);
    }
    if let Some(tool) = tool {
        if let Some(tool_api_key) = &tool.external_api_key {
            if !tool_api_key.is_empty() && tool_api_key == api_key {
                return Ok(true);
            }
        }
    }
    if let Some(global_key) = &config.toolguard.global_api_key {
        if !global_key.is_empty() && global_key == api_key {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn find_user_by_card(state: &AppState, card: &str) -> Result<Option<crate::models::User>, ApiError> {
    let config = state.config_manager.get_config();
    let profile_field = &config.toolguard.profile_field;
    state.db.find_user_by_profile_field(profile_field, card)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to query user by profile field: {}", e)))
}

async fn find_tool_by_toolguard_id(state: &AppState, toolguard_id: &str) -> Result<Option<crate::models::Tool>, ApiError> {
    // Try external_id first
    if let Ok(Some(tool)) = state.db.get_tool_by_external_id(toolguard_id) {
        return Ok(Some(tool));
    }
    // Fall back to UUID id if the value parses as one
    if let Ok(uuid) = Uuid::parse_str(toolguard_id) {
        if let Ok(Some(tool)) = state.db.get_tool_by_id(uuid) {
            return Ok(Some(tool));
        }
    }
    Ok(None)
}

async fn log_tool_access_denied(
    state: &AppState,
    user: Option<&crate::models::User>,
    toolguard_id: &str,
    reason: &str,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "toolguard_id": toolguard_id,
        "reason": reason,
        "card_provided": user.is_none(),
    });
    let audit_logger = state.audit_logger.clone();
    let user_id = user.map(|u| u.id);
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolAccessDenied,
            user_id, user_id, details, None, None,
        ).await;
    });
    Ok(())
}

async fn log_tool_activated(
    state: &AppState,
    user: &crate::models::User,
    tool_id: Uuid,
    toolguard_id: &str,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "tool_id": tool_id,
        "toolguard_id": toolguard_id,
        "action": "activated",
    });
    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolActivated,
            Some(user_id), Some(user_id), details, None, None,
        ).await;
    });
    Ok(())
}

async fn log_tool_deactivated(
    state: &AppState,
    user: &crate::models::User,
    tool_id: Uuid,
    toolguard_id: &str,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "tool_id": tool_id,
        "toolguard_id": toolguard_id,
        "action": "deactivated",
    });
    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolDeactivated,
            Some(user_id), Some(user_id), details, None, None,
        ).await;
    });
    Ok(())
}

async fn log_tool_usage(
    state: &AppState,
    user: &crate::models::User,
    tool_id: Uuid,
    toolguard_id: &str,
    seconds: f32,
    temperature: Option<f32>,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "tool_id": tool_id,
        "toolguard_id": toolguard_id,
        "seconds": seconds,
        "temperature": temperature,
        "duration_minutes": seconds / 60.0,
    });
    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    tokio::spawn(async move {
        let _ = audit_logger.log_event(
            crate::models::AuditEventType::ToolUsageLogged,
            Some(user_id), Some(user_id), details, None, None,
        ).await;
    });
    Ok(())
}
