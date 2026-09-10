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
        Self {
            status: "ok".to_string(),
            message: None,
            tool_on: None,
            tool_off: None,
        }
    }

    fn ok_with_message(message: impl Into<String>) -> Self {
        Self {
            status: "ok".to_string(),
            message: Some(message.into()),
            tool_on: None,
            tool_off: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            status: "error".to_string(),
            message: Some(message.into()),
            tool_on: None,
            tool_off: None,
        }
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

/// A power reading from a tool's controller (#43). The tool is named by its
/// toolguard/external id; every measurement is optional so an older or partial
/// firmware still parses. Authenticated like the other controller endpoints: a
/// device Bearer token, or the tool's `external_api_key` / the global key.
#[derive(Debug, Deserialize)]
pub struct PowerReportRequest {
    // Optional so an empty/partial body still deserializes: authentication (in
    // the handler) must be reached and answer 401 before a missing field can
    // answer 422. tool_id's presence is validated after the auth check.
    #[serde(default)]
    pub tool_id: Option<String>,
    #[serde(default)]
    pub draw_now: Option<bigdecimal::BigDecimal>,
    #[serde(default)]
    pub voltage_now: Option<bigdecimal::BigDecimal>,
    #[serde(default)]
    pub max_voltage: Option<bigdecimal::BigDecimal>,
    #[serde(default)]
    pub amperage_limit: Option<bigdecimal::BigDecimal>,
    /// True when the controller shut ITSELF off after exceeding its own
    /// over-current limit (#44). Locks just this tool -- the circuit is fine.
    #[serde(default)]
    pub self_tripped: Option<bool>,
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
    /// True for a metered tool under `actuation_mode = OnlineSynchronous`: the
    /// edge must call the server before energizing it (rather than deciding from
    /// the cached allow-list). `#[serde(default)]` so an older edge/payload
    /// without the field still parses (defaults to the offline-capable path).
    #[serde(default)]
    pub requires_online: bool,
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
        .route("/power-report", post(power_report))
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

    let inuse_tools = state.db.get_inuse_tools().map_err(|e| {
        ApiError::InternalServerError(format!("Failed to query InUse tools: {}", e))
    })?;

    let count = inuse_tools.len();
    for tool in inuse_tools {
        state
            .db
            .update_tool_status(tool.id, &crate::models::ToolStatus::Idle)
            .map_err(|e| {
                ApiError::InternalServerError(format!("Failed to reset tool {}: {}", tool.id, e))
            })?;

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
        state.db.create_tool_event(&event).map_err(|e| {
            ApiError::InternalServerError(format!("Failed to create tool event: {}", e))
        })?;

        // Metered tool billing (Phase 2): a tool force-reset at boot never got a
        // clean tool-off, so settle its open session as abandoned -- charge the
        // usage reported so far and release the hold.
        if let Some(billing) = &state.tool_billing {
            if billing.enabled() && billing.tool_is_metered(&tool) {
                if let Err(e) = billing.settle_open_session_for_tool(&tool, "abandoned") {
                    tracing::error!("tool billing: settle on boot-reset failed: {e}");
                }
            }
        }
    }

    if count > 0 {
        tracing::info!(
            "Boot-reset: {} tool(s) reset to idle by device {}",
            count,
            device_id
        );

        let audit_logger = state.audit_logger.clone();
        let details = serde_json::json!({
            "device_id": device_id,
            "tools_reset": count,
            "reason": "boot-reset",
        });
        tokio::spawn(async move {
            let _ = audit_logger
                .log_event(
                    crate::models::AuditEventType::ToolDeactivated,
                    None,
                    None,
                    details,
                    None,
                    None,
                )
                .await;
        });

        broadcast_toolguard_state(&state).await;
    }

    Ok(Json(ToolGuardResponse::ok_with_message(format!(
        "{} tool(s) reset to idle",
        count
    ))))
}

/// GET /api/toolguard/tool-on
async fn tool_on(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(req): Query<ToolRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    authorize_toolguard(&state, &headers, req.api_key.as_deref(), &req.tool_id).await?;

    tracing::info!(
        "Tool on request: card={}, tool_id={}",
        req.card,
        req.tool_id
    );

    let tool_for_key_check = find_tool_by_toolguard_id(&state, &req.tool_id).await?;
    if !validate_api_key(
        &state,
        req.api_key.as_deref().unwrap_or(""),
        tool_for_key_check.as_ref(),
    )
    .await?
    {
        log_tool_access_denied(&state, None, &req.tool_id, "Invalid or missing API key").await?;
        return Ok(Json(ToolGuardResponse::tool_denied(
            "Invalid or missing API key",
        )));
    }

    let user = match resolve_card(&state, &req.card).await? {
        crate::models::CardResolution::Active { user, card } => {
            // Record the presentation on the card that opened the tool.
            if let Some(c) = card {
                let _ = state.db.touch_card_last_used(c.id);
            }
            user
        }
        crate::models::CardResolution::Revoked { user, card } => {
            // Known credential deliberately not active — deny, but raise the
            // distinct fraud-signal event (a found/stolen card, or a member on
            // hold), separate from the quiet unknown-card denial below.
            log_revoked_card_presented(&state, &user, &card, &req.tool_id).await?;
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Card revoked").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Card not authorized")));
        }
        crate::models::CardResolution::Unknown => {
            log_tool_access_denied(&state, None, &req.tool_id, "Unknown card").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Unknown card")));
        }
    };

    if !user.is_active {
        log_tool_access_denied(&state, Some(&user), &req.tool_id, "User is not active").await?;
        return Ok(Json(ToolGuardResponse::tool_denied("User is not active")));
    }

    let tool = match tool_for_key_check {
        Some(t) => t,
        None => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool not found").await?;
            return Ok(Json(ToolGuardResponse::tool_denied("Tool not found")));
        }
    };

    match tool.status {
        crate::models::ToolStatus::Idle => {}
        crate::models::ToolStatus::InUse => {
            log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is already in use")
                .await?;
            return Ok(Json(ToolGuardResponse::tool_denied(
                "Tool is already in use",
            )));
        }
        crate::models::ToolStatus::Maintenance => {
            log_tool_access_denied(
                &state,
                Some(&user),
                &req.tool_id,
                "Tool is under maintenance",
            )
            .await?;
            return Ok(Json(ToolGuardResponse::tool_denied(
                "Tool is under maintenance",
            )));
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

    // One shared rule: training-step completion, an active waiver, or the tool
    // not being access-controlled. See DatabaseManager::user_is_authorized_for_tool.
    let authorized = state
        .db
        .user_is_authorized_for_tool(user.id, tool.id, tool.requires_training)
        .map_err(|e| {
            ApiError::InternalServerError(format!("Failed to check tool authorization: {}", e))
        })?;
    if !authorized {
        log_tool_access_denied(&state, Some(&user), &req.tool_id, "Training required").await?;
        return Ok(Json(ToolGuardResponse::tool_denied("Training required")));
    }

    // Metered tool billing (Phase 2). Money is the LAST gate: training passed
    // above, so a hold is never placed for a member who was not eligible to use
    // the tool. A metered tool must also authenticate with its own per-tool key
    // (not the shared global key), so one leaked key can't run up charges on
    // every tool.
    if let Some(billing) = &state.tool_billing {
        if billing.enabled() && billing.tool_is_metered(&tool) {
            if !metered_key_ok(req.api_key.as_deref(), &tool) {
                log_tool_access_denied(
                    &state,
                    Some(&user),
                    &req.tool_id,
                    "Metered tool requires its own API key",
                )
                .await?;
                return Ok(Json(ToolGuardResponse::tool_denied(
                    "Metered tool requires its own API key",
                )));
            }
            match billing.open_session(&user, &tool).map_err(ApiError::from)? {
                crate::tool_billing::ActivationOutcome::Authorized(_) => {}
                crate::tool_billing::ActivationOutcome::Denied(reason) => {
                    log_tool_access_denied(&state, Some(&user), &req.tool_id, &reason).await?;
                    return Ok(Json(ToolGuardResponse::tool_denied(&reason)));
                }
            }
        }
    }

    state
        .db
        .update_tool_status(tool.id, &crate::models::ToolStatus::InUse)
        .map_err(|e| {
            ApiError::InternalServerError(format!("Failed to update tool status: {}", e))
        })?;

    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "activated".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: Some(crate::models::ToolStatus::InUse),
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!(
            "Activated via ToolGuard (tool_id: {})",
            req.tool_id
        )),
        scan_data: Some(serde_json::json!({
            "toolguard_id": req.tool_id,
            "card": req.card,
        })),
    };
    state.db.create_tool_event(&event).map_err(|e| {
        ApiError::InternalServerError(format!("Failed to create tool event: {}", e))
    })?;

    log_tool_activated(&state, &user, tool.id, &req.tool_id).await?;

    // Broadcast updated state over MQTT
    broadcast_toolguard_state(&state).await;

    Ok(Json(ToolGuardResponse::tool_authorized()))
}

/// GET /api/toolguard/tool-off
async fn tool_off(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(req): Query<ToolRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    authorize_toolguard(&state, &headers, req.api_key.as_deref(), &req.tool_id).await?;

    tracing::info!(
        "Tool off request: card={}, tool_id={}",
        req.card,
        req.tool_id
    );

    let tool = find_tool_by_toolguard_id(&state, &req.tool_id).await?;
    if !validate_api_key(&state, req.api_key.as_deref().unwrap_or(""), tool.as_ref()).await? {
        log_tool_access_denied(&state, None, &req.tool_id, "Invalid or missing API key").await?;
        return Ok(Json(ToolGuardResponse::error("Invalid or missing API key")));
    }

    // Settle/report path: an already-open session must still close even if the
    // card was disabled or released mid-use, so both Active and Revoked resolve
    // to the member here; only a truly unknown code is rejected. (The revoked
    // fraud signal is raised at tool-on, where access is actually granted.)
    let user = match resolve_card(&state, &req.card).await? {
        crate::models::CardResolution::Active { user, .. }
        | crate::models::CardResolution::Revoked { user, .. } => user,
        crate::models::CardResolution::Unknown => {
            return Ok(Json(ToolGuardResponse::error("Unknown card")))
        }
    };

    let tool = match tool {
        Some(t) => t,
        None => return Ok(Json(ToolGuardResponse::error("Tool not found"))),
    };

    state
        .db
        .update_tool_status(tool.id, &crate::models::ToolStatus::Idle)
        .map_err(|e| {
            ApiError::InternalServerError(format!("Failed to update tool status: {}", e))
        })?;

    // Metered tool billing (Phase 2): settle the open session on stop -- post
    // the actual charge and release the hold; the broadcast below then refreshes
    // the allow-list with the freed balance. The tool is released regardless of
    // the key (safety); a mis-keyed stop defers the settle to the sweep rather
    // than trusting it.
    if let Some(billing) = &state.tool_billing {
        if billing.enabled() && billing.tool_is_metered(&tool) {
            if metered_key_ok(req.api_key.as_deref(), &tool) {
                if let Err(e) = billing.settle_open_session_for_tool(&tool, "settled") {
                    tracing::error!("tool billing: settle on tool-off failed: {e}");
                }
            } else {
                tracing::warn!(
                    "tool billing: metered tool-off without the tool's key; deferring \
                     settle to the sweep for tool_id={}",
                    req.tool_id
                );
            }
        }
    }

    use crate::models::NewToolEvent;
    let event = NewToolEvent {
        tool_id: tool.id,
        event_type: "deactivated".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: Some(crate::models::ToolStatus::Idle),
        user_id: Some(user.id),
        actor_id: Some(user.id),
        notes: Some(format!(
            "Deactivated via ToolGuard (tool_id: {})",
            req.tool_id
        )),
        scan_data: Some(serde_json::json!({
            "toolguard_id": req.tool_id,
            "card": req.card,
        })),
    };
    state.db.create_tool_event(&event).map_err(|e| {
        ApiError::InternalServerError(format!("Failed to create tool event: {}", e))
    })?;

    log_tool_deactivated(&state, &user, tool.id, &req.tool_id).await?;

    broadcast_toolguard_state(&state).await;

    Ok(Json(ToolGuardResponse::tool_off_ok()))
}

/// GET /api/toolguard/tool-log
async fn tool_log(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(req): Query<ToolLogRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    authorize_toolguard(&state, &headers, req.api_key.as_deref(), &req.tool_id).await?;

    tracing::info!(
        "Tool log request: card={}, tool_id={}, seconds={}, temp={:?}",
        req.card,
        req.tool_id,
        req.seconds,
        req.temperature
    );

    let tool = find_tool_by_toolguard_id(&state, &req.tool_id).await?;
    if !validate_api_key(&state, req.api_key.as_deref().unwrap_or(""), tool.as_ref()).await? {
        log_tool_access_denied(&state, None, &req.tool_id, "Invalid or missing API key").await?;
        return Ok(Json(ToolGuardResponse::error("Invalid or missing API key")));
    }

    // Settle/report path: an already-open session must still close even if the
    // card was disabled or released mid-use, so both Active and Revoked resolve
    // to the member here; only a truly unknown code is rejected. (The revoked
    // fraud signal is raised at tool-on, where access is actually granted.)
    let user = match resolve_card(&state, &req.card).await? {
        crate::models::CardResolution::Active { user, .. }
        | crate::models::CardResolution::Revoked { user, .. } => user,
        crate::models::CardResolution::Unknown => {
            return Ok(Json(ToolGuardResponse::error("Unknown card")))
        }
    };

    let tool = match tool {
        Some(t) => t,
        None => return Ok(Json(ToolGuardResponse::error("Tool not found"))),
    };

    // Metered tool billing (Phase 2): a billable report must use the tool's own
    // key and correlate to an open activation session; a report with no open
    // session (or a negative/absurd value) is rejected rather than billed. The
    // seconds are validated + capped inside record_usage / at settle.
    if let Some(billing) = &state.tool_billing {
        if billing.enabled() && billing.tool_is_metered(&tool) {
            if !metered_key_ok(req.api_key.as_deref(), &tool) {
                log_tool_access_denied(
                    &state,
                    Some(&user),
                    &req.tool_id,
                    "Metered tool requires its own API key",
                )
                .await?;
                return Ok(Json(ToolGuardResponse::error(
                    "Metered tool requires its own API key",
                )));
            }
            let applied = billing
                .record_usage(tool.id, req.seconds)
                .map_err(ApiError::from)?;
            if !applied {
                log_tool_access_denied(
                    &state,
                    Some(&user),
                    &req.tool_id,
                    "Usage report without an open session (or invalid seconds)",
                )
                .await?;
                return Ok(Json(ToolGuardResponse::error(
                    "No open session for this tool",
                )));
            }
        }
    }

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
    state.db.create_tool_event(&event).map_err(|e| {
        ApiError::InternalServerError(format!("Failed to create tool event: {}", e))
    })?;

    log_tool_usage(
        &state,
        &user,
        tool.id,
        &req.tool_id,
        req.seconds,
        req.temperature,
    )
    .await?;

    Ok(Json(ToolGuardResponse::ok_with_message("Usage logged")))
}

/// POST /api/toolguard/power-report - record a tool's latest power reading.
///
/// Device firmware is out of scope for #43; this is the server end of the seam
/// firmware engineers target (via the edge, which relays a firmware report up).
/// It stores the latest reading per tool -- the hot-path store the per-circuit
/// aggregation (#44) sums over -- and the firmware-declared max_voltage /
/// amperage_limit. It does NOT energize or interrupt anything.
async fn power_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PowerReportRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    // Authenticate before validating the body, so a credential-less request is
    // refused with 401 rather than a 422 about the missing tool_id.
    let toolguard_id = req.tool_id.as_deref().unwrap_or("");
    authorize_toolguard(&state, &headers, req.api_key.as_deref(), toolguard_id).await?;

    if toolguard_id.is_empty() {
        return Err(ApiError::BadRequest("tool_id is required".to_string()));
    }

    let tool = find_tool_by_toolguard_id(&state, toolguard_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    let reading = crate::models::NewToolPowerState {
        tool_id: tool.id,
        last_draw_amps: req.draw_now,
        last_voltage: req.voltage_now,
        reported_max_voltage: req.max_voltage,
        reported_amperage_limit: req.amperage_limit,
        last_reported_at: chrono::Utc::now(),
    };
    state.db.upsert_tool_power_state(&reading)?;

    // Fault-scoped trip (#44). A firmware self-trip locks just this tool; any
    // other report drives the server-side circuit aggregation, which trips the
    // whole circuit if its summed draw now exceeds the limit.
    let mut lockout_changed = false;
    if req.self_tripped == Some(true) {
        state
            .db
            .engage_tool_lockout(
                tool.id,
                "firmware self-trip: device exceeded its own over-current limit",
            )
            .map_err(ApiError::from)?;
        power_audit(
            &state,
            crate::models::AuditEventType::FirmwareSelftripReported,
            serde_json::json!({ "tool_id": tool.id, "external_id": toolguard_id }),
        );
        power_audit(
            &state,
            crate::models::AuditEventType::EmergencyLockoutEngaged,
            serde_json::json!({ "scope": "tool", "tool_id": tool.id, "source": "firmware_selftrip" }),
        );
        lockout_changed = true;
    } else {
        let tripped = state
            .db
            .evaluate_circuit_overages()
            .map_err(ApiError::from)?;
        for (cid, total, limit) in &tripped {
            power_audit(
                &state,
                crate::models::AuditEventType::CircuitOverageShutoff,
                serde_json::json!({
                    "circuit_id": cid,
                    "total_draw_amps": total.to_string(),
                    "amperage_limit": limit.to_string(),
                }),
            );
            power_audit(
                &state,
                crate::models::AuditEventType::EmergencyLockoutEngaged,
                serde_json::json!({ "scope": "circuit", "circuit_id": cid, "source": "server_aggregate" }),
            );
            lockout_changed = true;
        }
    }

    // Re-publish the allow-list so a locked circuit's / tool's tools drop off
    // every edge's cached authorization set (the distribution mechanism a
    // shut-off rides -- the gate already denies them on the next request).
    if lockout_changed {
        broadcast_toolguard_state(&state).await;
    }

    Ok(Json(ToolGuardResponse::ok_with_message("Power reported")))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Write a power interrupt/lockout audit event. These are system-triggered (a
/// controller's report tripped a limit), so there is no human actor -- the
/// staff who clears a lockout is recorded on the re-enable path in `api::power`.
fn power_audit(state: &AppState, event: crate::models::AuditEventType, data: serde_json::Value) {
    let log = crate::models::NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: None,
        actor_id: None,
        event_data: data,
        ip_address: None,
        user_agent: None,
    };
    if let Err(e) = state.db.create_audit_log(&log) {
        tracing::error!("Failed to write power audit log {}: {}", event.as_str(), e);
    }
}

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

    let (device_id, _) = state
        .db
        .find_device_by_auth_token(token)
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
    // When tool billing is on, a config snapshot lets the DB layer drop metered
    // tools a member can't afford (or isn't a member for) from each user's
    // authorized list -- the per-user affordability filter that makes edge-local
    // actuation safe -- and flag metered tools that need online actuation.
    let metered_gate = state.tool_billing.as_ref().map(|svc| svc.gate());
    let (mut users, mut tools) = state
        .db
        .get_toolguard_sync_data(profile_field, metered_gate.as_ref())
        .map_err(|e| ApiError::InternalServerError(format!("Failed to build sync data: {}", e)))?;

    // Apply schedule gating: any tool whose attached schedule is closed
    // right now is removed from every user's authorized list, and then
    // dropped from the top-level tool list since nobody can use it.
    let closed_tool_ids = closed_tool_ids_now(state)?;
    if !closed_tool_ids.is_empty() {
        for u in users.iter_mut() {
            u.authorized_tool_ids
                .retain(|tid| !closed_tool_ids.contains(tid));
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
fn closed_tool_ids_now(state: &AppState) -> Result<std::collections::HashSet<Uuid>, ApiError> {
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
            Ok(payload) => match serde_json::to_vec(&payload) {
                Ok(bytes) => {
                    if let Err(e) = mqtt_service.publish_toolguard_state(device_id, bytes) {
                        tracing::warn!(
                            "Failed to publish toolguard state to device {}: {}",
                            device_id,
                            e
                        );
                    }
                }
                Err(e) => tracing::warn!("Failed to serialize toolguard payload: {}", e),
            },
            Err(e) => tracing::warn!(
                "Failed to build sync payload for device {}: {}",
                device_id,
                e
            ),
        }
    }
}

/// Authenticate a ToolGuard tool operation.
///
/// These endpoints energise and de-energise physical machinery and are
/// reachable by URL, so until this existed anyone who could reach the server
/// could turn a tool on for any card by visiting a link. `sync` and
/// `boot_reset` beside them authenticated correctly; `tool_on`, `tool_off` and
/// `tool_log` did not, and the mechanism meant to protect them —
/// [`validate_api_key`], plus the `api_key` field on the request types — was
/// fully written and called from nowhere.
///
/// Two accepted credentials, in cost order:
///
/// 1. A registered device's Bearer token. This is the normal path: the edge
///    already sends `bearer_auth` on all three of these calls, so it needed no
///    change to keep working — the server was simply discarding a credential
///    it was being given.
/// 2. A per-tool `external_api_key` or the global `toolguard.global_api_key`,
///    for controllers that authenticate that way instead. Checked second
///    because it needs a database round-trip to resolve the tool first.
async fn authorize_toolguard(
    state: &AppState,
    headers: &HeaderMap,
    api_key: Option<&str>,
    toolguard_id: &str,
) -> Result<(), ApiError> {
    match extract_device_auth(state, headers).await {
        Ok(_) => return Ok(()),
        // A database fault must stay a database fault. Folding it into "not
        // authenticated" would report an outage as a credential problem and
        // send whoever is holding a dead tool looking in the wrong place.
        Err(e @ ApiError::InternalServerError(_)) => return Err(e),
        Err(_) => {}
    }

    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        let tool = find_tool_by_toolguard_id(state, toolguard_id).await?;
        if validate_api_key(state, key, tool.as_ref()).await? {
            return Ok(());
        }
    }

    tracing::warn!(
        "Rejected unauthenticated ToolGuard request for tool_id={}",
        toolguard_id
    );
    Err(ApiError::Unauthorized(
        "ToolGuard operations require a registered device token or a valid API key".to_string(),
    ))
}

/// Metered tools must authenticate with their OWN `external_api_key`, not the
/// shared global key (and not a bare device token). This binds a billable report
/// to the specific tool's secret, so one leaked global key cannot post charges
/// for every tool. A metered tool with no key set can never satisfy this -- by
/// design, an unbillable/forgeable metered tool is refused rather than trusted.
fn metered_key_ok(api_key: Option<&str>, tool: &crate::models::Tool) -> bool {
    match (api_key, tool.external_api_key.as_deref()) {
        (Some(provided), Some(tool_key)) => !tool_key.is_empty() && provided == tool_key,
        _ => false,
    }
}

async fn validate_api_key(
    state: &AppState,
    api_key: &str,
    tool: Option<&crate::models::Tool>,
) -> Result<bool, ApiError> {
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

async fn resolve_card(
    state: &AppState,
    card: &str,
) -> Result<crate::models::CardResolution, ApiError> {
    let config = state.config_manager.get_config();
    let profile_field = &config.toolguard.profile_field;
    state
        .db
        .resolve_card(profile_field, card)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to resolve card: {}", e)))
}

/// Emit the distinct high-signal audit event for a known-but-revoked
/// (disabled/released) card presented at a tool. Denial is handled separately;
/// this event exists purely as a hook for later fraud alerting.
async fn log_revoked_card_presented(
    state: &AppState,
    user: &crate::models::User,
    card: &crate::models::UserCard,
    toolguard_id: &str,
) -> Result<(), ApiError> {
    let details = serde_json::json!({
        "toolguard_id": toolguard_id,
        "card_code": card.code,
        "card_status": card.status,
        "context": "tool",
    });
    let audit_logger = state.audit_logger.clone();
    let user_id = user.id;
    tokio::spawn(async move {
        let _ = audit_logger
            .log_event(
                crate::models::AuditEventType::RevokedCardPresented,
                Some(user_id),
                Some(user_id),
                details,
                None,
                None,
            )
            .await;
    });
    Ok(())
}

async fn find_tool_by_toolguard_id(
    state: &AppState,
    toolguard_id: &str,
) -> Result<Option<crate::models::Tool>, ApiError> {
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
        let _ = audit_logger
            .log_event(
                crate::models::AuditEventType::ToolAccessDenied,
                user_id,
                user_id,
                details,
                None,
                None,
            )
            .await;
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
        let _ = audit_logger
            .log_event(
                crate::models::AuditEventType::ToolActivated,
                Some(user_id),
                Some(user_id),
                details,
                None,
                None,
            )
            .await;
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
        let _ = audit_logger
            .log_event(
                crate::models::AuditEventType::ToolDeactivated,
                Some(user_id),
                Some(user_id),
                details,
                None,
                None,
            )
            .await;
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
        let _ = audit_logger
            .log_event(
                crate::models::AuditEventType::ToolUsageLogged,
                Some(user_id),
                Some(user_id),
                details,
                None,
                None,
            )
            .await;
    });
    Ok(())
}
