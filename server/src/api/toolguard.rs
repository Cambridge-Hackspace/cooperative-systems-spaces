use axum::body::Bytes;
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
}

/// What actually arrives on the wire, before anything is known to be present.
///
/// Every field is optional and the struct is validated *after* authentication,
/// which preserves the contract `FIRMWARE.md` states and the 401 matrix
/// asserts: a request with no credential is **401**, not a 4xx about its body.
/// A caller holding nothing must learn exactly one thing -- that it holds
/// nothing -- and must never be able to tell a well-formed request from a
/// malformed one, because that difference is a probing oracle.
///
/// This is why the handlers take `Bytes` rather than `Json<..>`. Optional
/// fields alone are not enough: `Json` is a whole-request extractor that
/// rejects a missing or wrong `Content-Type` with **415** before the handler
/// body runs, which puts the same leak one extractor further down. The e2e
/// contract matrix caught exactly that. `Bytes` never rejects, so
/// authentication genuinely runs first.
///
/// Same reasoning as [`PowerReportRequest`] and [`PowerTripRequest`].
#[derive(Debug, Default, Deserialize)]
pub struct ToolRequestWire {
    #[serde(default)]
    pub card: Option<String>,
    #[serde(default)]
    pub tool_id: Option<String>,
}

impl ToolRequestWire {
    /// The tool id as authentication needs it, before validation.
    ///
    /// An absent tool id reads as empty, which `authorize_toolguard` treats as a
    /// device-wide operation -- so a body with no tool_id can never authorize a
    /// per-tool one.
    fn borrowed(&self) -> &str {
        self.tool_id.as_deref().unwrap_or("")
    }

    fn validated(self) -> Result<ToolRequest, ApiError> {
        match (self.card, self.tool_id) {
            (Some(card), Some(tool_id)) => Ok(ToolRequest { card, tool_id }),
            _ => Err(ApiError::BadRequest(
                "card and tool_id are required".to_string(),
            )),
        }
    }
}

/// A power reading from a tool's controller (#43). The tool is named by its
/// toolguard/external id; every measurement is optional so an older or partial
/// firmware still parses. Authenticated like the other controller endpoints: a
/// registered device's Bearer token, bound to the tool it names (#101).
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
    /// The module's own view of its output: is the relay closed? (#84)
    ///
    /// Absent means "this module does not report relay state", which is not the
    /// same as `Some(false)`. The detector treats unknown as unknown rather than
    /// as off, so a plug that cannot answer does not silently disarm it.
    #[serde(default)]
    pub relay_on: Option<bool>,
    /// Which module sent this reading (#161).
    ///
    /// The MQTT twin `toolguard/request/power` has carried this since #83,
    /// because the edge needs it to know a module is alive before it will grant
    /// a lease. The HTTP form did not, so when an edge forwarded a module's
    /// report the only device the server could see was the EDGE -- and
    /// `space_devices.last_seen_at` is written nowhere else. A module behind an
    /// edge therefore had no liveness at all from the server's side: the bypass
    /// sweep recorded it silent on its first pass and then sat at NoChange
    /// forever, so it could never report the transition it exists for.
    ///
    /// Optional, so an older edge keeps working -- it simply contributes no
    /// liveness, which is the state before this field existed.
    #[serde(default)]
    pub device_id: Option<uuid::Uuid>,
}

/// Request parameters for tool logging
#[derive(Debug, Deserialize)]
pub struct ToolLogRequest {
    pub card: String,
    pub tool_id: String,
    pub seconds: f32,
    pub temperature: Option<f32>,
}

/// The wire form of [`ToolLogRequest`]; see [`ToolRequestWire`] for why every
/// field is optional.
#[derive(Debug, Default, Deserialize)]
pub struct ToolLogRequestWire {
    #[serde(default)]
    pub card: Option<String>,
    #[serde(default)]
    pub tool_id: Option<String>,
    #[serde(default)]
    pub seconds: Option<f32>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

impl ToolLogRequestWire {
    /// The tool id as authentication needs it, before validation. See
    /// [`ToolRequestWire::borrowed`].
    fn borrowed(&self) -> &str {
        self.tool_id.as_deref().unwrap_or("")
    }

    fn validated(self) -> Result<ToolLogRequest, ApiError> {
        match (self.card, self.tool_id, self.seconds) {
            (Some(card), Some(tool_id), Some(seconds)) => Ok(ToolLogRequest {
                card,
                tool_id,
                seconds,
                temperature: self.temperature,
            }),
            _ => Err(ApiError::BadRequest(
                "card, tool_id and seconds are required".to_string(),
            )),
        }
    }
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
    /// Hex `argon2id(cards.device_pepper, card)` (#109) -- **not** the card.
    ///
    /// A device authorizes offline, so it holds whatever the comparison needs;
    /// it used to hold the card identifier itself, in a file on a plug screwed
    /// to a wall. It now holds this, and hashes the swipe the same way.
    ///
    /// Empty when the deployment has configured no card keys, which is the
    /// pre-migration state and means no device can authorize offline.
    pub profile_field_digest: String,
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
        .route("/tool-on", post(tool_on))
        .route("/tool-off", post(tool_off))
        .route("/tool-log", post(tool_log))
        .route("/sync", get(sync))
        .route("/boot-reset", post(boot_reset))
        .route("/power-report", post(power_report))
        .route("/power-state", get(power_state))
        .route("/module-state", get(module_state))
        .route("/power-trip", post(power_trip))
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

        let details = serde_json::json!({
            "device_id": device_id,
            "tools_reset": count,
            "reason": "boot-reset",
        });
        if let Err(e) = state
            .audit_logger
            .log_event(
                crate::models::AuditEventType::ToolDeactivated,
                None,
                None,
                details,
                None,
                None,
            )
            .await
        {
            tracing::error!("Failed to write audit event: {}", e);
        }

        broadcast_toolguard_state(&state).await;
    }

    Ok(Json(ToolGuardResponse::ok_with_message(format!(
        "{} tool(s) reset to idle",
        count
    ))))
}

/// POST /api/toolguard/tool-on
async fn tool_on(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let parsed: Option<ToolRequestWire> = serde_json::from_slice(&body).ok();
    let tool_id = parsed.as_ref().map(ToolRequestWire::borrowed).unwrap_or("");
    let device_id = authorize_toolguard(&state, &headers, tool_id).await?;
    let req = parsed
        .ok_or_else(|| ApiError::BadRequest("body must be a JSON object".to_string()))?
        .validated()?;

    // The card is deliberately absent from this line (#107). It identifies a
    // person, this log is written on every swipe, and the file is collected as
    // a CI artifact and readable by anyone who can reach the container host.
    tracing::info!("Tool on request: tool_id={}", req.tool_id);

    // #120 (#14): authentication already happened in authorize_toolguard above --
    // a device token bound to this tool. #101 slice 6 retired the API-key
    // alternatives, so there is one credential and one place that checks it.
    // Metered tools face a stricter, narrower gate below (metered_device_ok): the
    // `power` binding, not merely any binding.
    let tool_lookup = find_tool_by_toolguard_id(&state, &req.tool_id).await?;

    // The card's id travels with the user; its *code* does not. The id says
    // which credential was presented -- everything an operator reading the
    // event needs -- without writing a person's physical identifier into a row
    // that outlives the session and lands in every backup (#107, #108).
    let (user, card_id) = match resolve_card(&state, &req.card).await? {
        crate::models::CardResolution::Active { user, card } => {
            // Record the presentation on the card that opened the tool.
            let card_id = card.as_ref().map(|c| c.id);
            if let Some(c) = card {
                let _ = state.db.touch_card_last_used(c.id);
            }
            (user, card_id)
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

    let tool = match tool_lookup {
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
    // the tool. A metered tool must additionally be addressed by a device bound
    // to it in the `power` role, so that a charge is attributable to the thing
    // that actually switches the machine rather than to anything holding a
    // valid token.
    //
    // The denial text used to say "requires its own API key", which outlived the
    // mechanism: per-tool and global API keys were retired in #101 and the field
    // is ignored. A firmware author reading that message went looking for a
    // credential that no longer exists, which is why it now names the binding.
    if let Some(billing) = &state.tool_billing {
        if billing.enabled() && billing.tool_is_metered(&tool) {
            if !metered_device_ok(&state, device_id, &tool).await? {
                log_tool_access_denied(
                    &state,
                    Some(&user),
                    &req.tool_id,
                    "Metered tool requires a power-bound device",
                )
                .await?;
                return Ok(Json(ToolGuardResponse::tool_denied(
                    "Metered tool requires a power-bound device",
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

    // #120/M2: atomic Idle->InUse. If another request won the race between the
    // status check above and here, deny as already-in-use rather than writing a
    // duplicate activation and event. (For a metered tool the open-session unique
    // index already blocked the double; this closes the window for non-metered
    // tools, which have no session to serialize on.)
    let activated = state
        .db
        .try_activate_tool(tool.id)
        .map_err(|e| ApiError::InternalServerError(format!("Failed to activate tool: {}", e)))?;
    if !activated {
        log_tool_access_denied(&state, Some(&user), &req.tool_id, "Tool is already in use").await?;
        return Ok(Json(ToolGuardResponse::tool_denied(
            "Tool is already in use",
        )));
    }

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
            "card_id": card_id,
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

/// POST /api/toolguard/tool-off
async fn tool_off(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let parsed: Option<ToolRequestWire> = serde_json::from_slice(&body).ok();
    let tool_id = parsed.as_ref().map(ToolRequestWire::borrowed).unwrap_or("");
    let device_id = authorize_toolguard(&state, &headers, tool_id).await?;
    let req = parsed
        .ok_or_else(|| ApiError::BadRequest("body must be a JSON object".to_string()))?
        .validated()?;

    tracing::info!("Tool off request: tool_id={}", req.tool_id);

    let tool = find_tool_by_toolguard_id(&state, &req.tool_id).await?;
    // #120 (#14): authenticated by authorize_toolguard above; the dead
    // double-auth gate that stood here (an API key required even from an
    // already-authenticated bound device) is removed. See tool_on.

    // Settle/report path: an already-open session must still close even if the
    // card was disabled or released mid-use, so both Active and Revoked resolve
    // to the member here; only a truly unknown code is rejected. (The revoked
    // fraud signal is raised at tool-on, where access is actually granted.)
    let (user, card_id) = match resolve_card(&state, &req.card).await? {
        crate::models::CardResolution::Active { user, card } => {
            let card_id = card.as_ref().map(|c| c.id);
            (user, card_id)
        }
        crate::models::CardResolution::Revoked { user, card } => (user, Some(card.id)),
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
            if metered_device_ok(&state, device_id, &tool).await? {
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
            "card_id": card_id,
        })),
    };
    state.db.create_tool_event(&event).map_err(|e| {
        ApiError::InternalServerError(format!("Failed to create tool event: {}", e))
    })?;

    log_tool_deactivated(&state, &user, tool.id, &req.tool_id).await?;

    broadcast_toolguard_state(&state).await;

    Ok(Json(ToolGuardResponse::tool_off_ok()))
}

/// POST /api/toolguard/tool-log
async fn tool_log(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    let parsed: Option<ToolLogRequestWire> = serde_json::from_slice(&body).ok();
    let tool_id = parsed
        .as_ref()
        .map(ToolLogRequestWire::borrowed)
        .unwrap_or("");
    let device_id = authorize_toolguard(&state, &headers, tool_id).await?;
    let req = parsed
        .ok_or_else(|| ApiError::BadRequest("body must be a JSON object".to_string()))?
        .validated()?;

    tracing::info!(
        "Tool log request: tool_id={}, seconds={}, temp={:?}",
        req.tool_id,
        req.seconds,
        req.temperature
    );

    let tool = find_tool_by_toolguard_id(&state, &req.tool_id).await?;
    // #120 (#14): authenticated by authorize_toolguard above; the dead
    // double-auth gate that stood here (an API key required even from an
    // already-authenticated bound device) is removed. See tool_on.

    // Settle/report path: an already-open session must still close even if the
    // card was disabled or released mid-use, so both Active and Revoked resolve
    // to the member here; only a truly unknown code is rejected. (The revoked
    // fraud signal is raised at tool-on, where access is actually granted.)
    let (user, card_id) = match resolve_card(&state, &req.card).await? {
        crate::models::CardResolution::Active { user, card } => {
            let card_id = card.as_ref().map(|c| c.id);
            (user, card_id)
        }
        crate::models::CardResolution::Revoked { user, card } => (user, Some(card.id)),
        crate::models::CardResolution::Unknown => {
            return Ok(Json(ToolGuardResponse::error("Unknown card")))
        }
    };

    let tool = match tool {
        Some(t) => t,
        None => return Ok(Json(ToolGuardResponse::error("Tool not found"))),
    };

    // Metered tool billing (Phase 2): a billable report must come from a device
    // bound to this tool in the `power` role and correlate to an open activation
    // session; a report with no open session (or a negative/absurd value) is
    // rejected rather than billed. The seconds are validated + capped inside
    // record_usage / at settle.
    if let Some(billing) = &state.tool_billing {
        if billing.enabled() && billing.tool_is_metered(&tool) {
            if !metered_device_ok(&state, device_id, &tool).await? {
                log_tool_access_denied(
                    &state,
                    Some(&user),
                    &req.tool_id,
                    "Metered tool requires a power-bound device",
                )
                .await?;
                return Ok(Json(ToolGuardResponse::error(
                    "Metered tool requires a power-bound device",
                )));
            }
            // #120/M1: a billable usage report must come from the card of the
            // member who activated the session -- otherwise a found or stray card
            // could inflate someone else's billed usage. The open session (not the
            // tool) holds the activator. Note this guards *reporting*, not
            // stopping: tool-off stays open to any card so anyone can cut power
            // for safety.
            if let Some(session) = state
                .db
                .open_tool_session_for_tool(tool.id)
                .map_err(ApiError::from)?
            {
                if session.user_id != user.id {
                    log_tool_access_denied(
                        &state,
                        Some(&user),
                        &req.tool_id,
                        "Usage report from a card that did not activate this session",
                    )
                    .await?;
                    return Ok(Json(ToolGuardResponse::error(
                        "This card did not activate the tool",
                    )));
                }
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
            "card_id": card_id,
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
    let caller = authorize_power_report(&state, &headers, toolguard_id, req.device_id).await?;

    if toolguard_id.is_empty() {
        return Err(ApiError::BadRequest("tool_id is required".to_string()));
    }

    let tool = find_tool_by_toolguard_id(&state, toolguard_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    // Only meaningful for a relay; a module reporting for itself is its own
    // subject. Kept so the log distinguishes the two paths.
    if caller.relayed_for.is_some() {
        tracing::debug!(
            "power-report for tool {} relayed by edge {} on behalf of module {:?}",
            tool.id,
            caller.device_id,
            caller.relayed_for
        );
    }

    // #161: credit the MODULE that sent this reading with being alive, not just
    // the caller that relayed it.
    //
    // The trust story, stated because it is the one thing this introduces: the
    // caller is an authenticated device, and it is VOUCHING for a module it
    // heard from on its own local broker. That is weaker than the module
    // authenticating for itself -- which it cannot do, having no site-broker
    // credentials by design (FIRMWARE.md, "a module does not need credentials
    // for the site broker and generally should not have them"). The alternative
    // is what we had: no liveness for that class of device at all, and a
    // detector that silently answers about nothing. A compromised edge can
    // already lie about every reading in this payload; being able to also say
    // "the plug I coordinate is alive" adds no capability worth the blindness.
    //
    // Failure here is logged and ignored. A liveness refresh is not what the
    // caller asked for, and losing it must not fail a power report that the
    // fast-trip path depends on.
    if let Some(module_id) = req.device_id {
        match state.db.touch_device_last_seen(module_id) {
            Ok(true) => {
                tracing::debug!("power-report: refreshed last_seen for module {module_id}");
            }
            Ok(false) => {
                // Named rather than silent: a module reporting faithfully under
                // an id the server does not know will read as permanently
                // silent, and from either end that looks like a dead plug.
                tracing::warn!(
                    "power-report named device {module_id}, which is not a registered device; \
                     its liveness cannot be recorded"
                );
            }
            Err(e) => tracing::warn!("power-report: failed to refresh last_seen: {e}"),
        }
    }

    // #84: is this tool powered right now, by either oracle? The clock only
    // restarts when an unbroken run of being powered ends, so a tool that has
    // been on for an hour does not look freshly switched on at every report.
    let now = chrono::Utc::now();
    let draw_threshold = state
        .config_manager
        .get_config()
        .bypass
        .unauthorized_power_draw_amps;
    let drawing = {
        use bigdecimal::ToPrimitive;
        req.draw_now
            .as_ref()
            .and_then(|d| d.to_f64())
            .map(|a| a >= draw_threshold)
            .unwrap_or(false)
    };
    let powered = req.relay_on.unwrap_or(false) || drawing;
    let previous = state.db.get_tool_power_state(tool.id)?;
    let power_evidence_since = if powered {
        previous
            .as_ref()
            .and_then(|p| p.power_evidence_since)
            .or(Some(now))
    } else {
        None
    };

    let reading = crate::models::NewToolPowerState {
        tool_id: tool.id,
        last_draw_amps: req.draw_now,
        last_voltage: req.voltage_now,
        reported_max_voltage: req.max_voltage,
        reported_amperage_limit: req.amperage_limit,
        last_reported_at: now,
        last_relay_on: req.relay_on,
        power_evidence_since,
    };
    state.db.upsert_tool_power_state(&reading)?;

    // Optional history (#49): mirror the reading onto the Prometheus gauges.
    // No-op unless the submodule is enabled.
    {
        use bigdecimal::ToPrimitive;
        crate::power_metrics::record(
            &tool.id.to_string(),
            reading.last_draw_amps.as_ref().and_then(|d| d.to_f64()),
            reading.last_voltage.as_ref().and_then(|v| v.to_f64()),
        );
    }

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
    // shut-off rides -- the gate already denies them on the next request), and
    // the explicit power-state (#48) so a disconnected edge stays fail-secure.
    if lockout_changed {
        broadcast_toolguard_state(&state).await;
        broadcast_power_state(&state).await;
    }

    Ok(Json(ToolGuardResponse::ok_with_message("Power reported")))
}

/// No fields: the state polls authenticate by device Bearer token alone (#101
/// slice 6 retired the API-key query parameter). Kept as a type so the handlers'
/// signatures still document that a query string is accepted and ignored.
#[derive(Debug, Deserialize)]
pub struct PowerStateQuery {}

/// GET /api/toolguard/power-state - the lockout + topology snapshot the edge
/// caches (#48). Authenticated like the other controller endpoints -- a registered
/// device's Bearer token; the poll fallback for the MQTT `power/state` push.
async fn power_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(_q): Query<PowerStateQuery>,
) -> Result<Json<css_lib::wire::PowerStatePayload>, ApiError> {
    authorize_toolguard(&state, &headers, "").await?;
    let payload = state.db.power_state_snapshot().map_err(ApiError::from)?;
    Ok(Json(payload))
}

/// GET /api/toolguard/module-state - the module bindings + interlock snapshot
/// the edge coordinates from (#83). Authenticated like the other controller
/// endpoints; the poll fallback for the MQTT `module/state` push.
async fn module_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(_q): Query<PowerStateQuery>,
) -> Result<Json<css_lib::wire::ToolModuleStatePayload>, ApiError> {
    authorize_toolguard(&state, &headers, "").await?;
    let payload = state.db.module_state_snapshot().map_err(ApiError::from)?;
    Ok(Json(payload))
}

/// A circuit overload the edge detected locally (#48). Fields optional so an
/// unauthenticated request is refused 401 before a missing field is a 422.
#[derive(Debug, Deserialize)]
pub struct PowerTripRequest {
    #[serde(default)]
    pub circuit_id: Option<Uuid>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// POST /api/toolguard/power-trip - the edge_fast_trip ingest (#48). The edge
/// summed draw across its local devices on a circuit and tripped; the server
/// records the lockout authoritatively (`lockout_source = edge_fast_trip`) and
/// re-broadcasts. Authenticated like the other controller endpoints.
async fn power_trip(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PowerTripRequest>,
) -> Result<Json<ToolGuardResponse>, ApiError> {
    authorize_toolguard(&state, &headers, "").await?;

    let Some(circuit_id) = req.circuit_id else {
        return Err(ApiError::BadRequest("circuit_id is required".to_string()));
    };
    let reason = req
        .reason
        .unwrap_or_else(|| "edge-reported circuit overload".to_string());

    let engaged = state
        .db
        .engage_circuit_lockout(circuit_id, "edge_fast_trip", &reason)
        .map_err(ApiError::from)?;

    if engaged > 0 {
        power_audit(
            &state,
            crate::models::AuditEventType::CircuitOverageShutoff,
            serde_json::json!({ "circuit_id": circuit_id, "source": "edge_fast_trip" }),
        );
        power_audit(
            &state,
            crate::models::AuditEventType::EmergencyLockoutEngaged,
            serde_json::json!({ "scope": "circuit", "circuit_id": circuit_id, "source": "edge_fast_trip" }),
        );
        broadcast_power_state(&state).await;
        broadcast_toolguard_state(&state).await;
    }

    Ok(Json(ToolGuardResponse::ok_with_message("Circuit tripped")))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Push the power lockout + topology snapshot (#48) to every approved device.
/// Sibling of `broadcast_toolguard_state`; called whenever a lockout changes.
pub async fn broadcast_power_state(state: &AppState) {
    let Some(mqtt_service) = state.mqtt_service.clone() else {
        return;
    };
    let payload = match state.db.power_state_snapshot() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to build power-state snapshot: {}", e);
            return;
        }
    };
    let bytes = match serde_json::to_vec(&payload) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Failed to serialize power-state payload: {}", e);
            return;
        }
    };
    let devices = match state.db.list_approved_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to list devices for power-state broadcast: {}", e);
            return;
        }
    };
    for device_id in devices {
        if let Err(e) = mqtt_service.publish_to_device(
            device_id,
            css_lib::wire::kinds::POWER_STATE,
            bytes.clone(),
        ) {
            tracing::warn!(
                "Failed to publish power-state to device {}: {}",
                device_id,
                e
            );
        }
    }
}

/// Push the `module/state` snapshot (#83) to every approved device, mirroring
/// [`broadcast_power_state`]. A device that does not yet understand the topic
/// ignores it, which is what makes adding this safe ahead of the edge-side
/// coordinator.
pub async fn broadcast_module_state(state: &AppState) {
    let Some(mqtt_service) = state.mqtt_service.clone() else {
        return;
    };
    let payload = match state.db.module_state_snapshot() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to build module-state snapshot: {}", e);
            return;
        }
    };
    let bytes = match serde_json::to_vec(&payload) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Failed to serialize module-state payload: {}", e);
            return;
        }
    };
    let devices = match state.db.list_approved_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to list devices for module-state broadcast: {}", e);
            return;
        }
    };
    for device_id in devices {
        if let Err(e) = mqtt_service.publish_to_device(
            device_id,
            css_lib::wire::kinds::MODULE_STATE,
            bytes.clone(),
        ) {
            tracing::warn!(
                "Failed to publish module-state to device {}: {}",
                device_id,
                e
            );
        }
    }
}

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
        .get_toolguard_sync_data(
            device_id,
            profile_field,
            metered_gate.as_ref(),
            state.card_cipher.as_deref(),
        )
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
            None => continue, // No schedule = always open (intended).
        };
        // #120 (#122/low): a tool that *references* a schedule which cannot be
        // resolved (deleted mid-race, or invalid intervals) is locked, not left
        // open -- fail closed, consistent with the door path (#7). Only a tool
        // with no schedule at all is unconditionally open.
        let sched = match by_id.get(&sid) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    "Tool {} references missing schedule {}; locking it",
                    tool_id,
                    sid
                );
                closed.insert(tool_id);
                continue;
            }
        };
        let intervals = match crate::schedules::parse_intervals(&sched.intervals) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "Tool {} schedule {} has invalid intervals ({}); locking it",
                    tool_id,
                    sid,
                    e
                );
                closed.insert(tool_id);
                continue;
            }
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
/// the `api_key` field on the request types — was fully written and called from
/// nowhere.
///
/// One credential: a registered device's Bearer token, stored hashed since
/// #120/#14. For a per-tool operation the device must be *bound to that tool*
/// (#104, now `device_bindings`): a reader wired to one tool cannot energise
/// another with its own token. Device-wide operations (`sync`, `boot_reset` and
/// the state polls, called with an empty `toolguard_id`) need only a valid token.
///
/// #101 slice 6 retired the two alternatives. `toolguard.global_api_key` was a
/// single shared secret that opened every tool, and `tools.external_api_key` was
/// a per-tool secret kept in a plaintext column; both were accepted here, so the
/// weakest credential set the real bar. A device token is hashed at rest and
/// scoped by an explicit binding an administrator made, which is what the rest of
/// #101 made uniform.
///
/// Returns the authenticated device id, because the metered-billing gate needs to
/// know WHICH device asked -- see [`metered_device_ok`], which additionally
/// requires the `power` binding before a charge may be posted.
async fn authorize_toolguard(
    state: &AppState,
    headers: &HeaderMap,
    toolguard_id: &str,
) -> Result<uuid::Uuid, ApiError> {
    let device_id = match extract_device_auth(state, headers).await {
        Ok((device_id, _)) => device_id,
        // A database fault must stay a database fault. Folding it into "not
        // authenticated" would report an outage as a credential problem and
        // send whoever is holding a dead tool looking in the wrong place.
        Err(e @ ApiError::InternalServerError(_)) => return Err(e),
        Err(_) => {
            tracing::warn!(
                "Rejected unauthenticated ToolGuard request for tool_id={}",
                toolguard_id
            );
            return Err(ApiError::Unauthorized(
                "ToolGuard operations require a registered device token".to_string(),
            ));
        }
    };

    // #120 (#14): a device token authorizes device-wide operations (sync,
    // boot-reset, the state polls -- called with an empty toolguard_id)
    // unconditionally, but a per-tool operation only for a tool this device is
    // actually bound to (#104). A reader wired to one tool therefore cannot
    // energise another with its own valid token.
    if toolguard_id.is_empty() {
        return Ok(device_id);
    }
    if let Some(tool) = find_tool_by_toolguard_id(state, toolguard_id).await? {
        // `?` converts a DatabaseError through the classified From<DatabaseError>
        // impl rather than a bare 500 -- a genuine DB fault here is ours (500),
        // but the classification stays in the one place that owns it
        // (api/errors.rs), per the blanket-500 ratchet.
        if state.db.device_is_bound_to_tool(device_id, tool.id)? {
            return Ok(device_id);
        }
    }

    tracing::warn!(
        "Rejected ToolGuard request from device {} for tool_id={}: not bound to it",
        device_id,
        toolguard_id
    );
    Err(ApiError::Unauthorized(
        "This device is not bound to that tool".to_string(),
    ))
}

/// Who sent a power report, and on whose behalf.
pub struct PowerReportCaller {
    /// The authenticated device that made the request.
    pub device_id: uuid::Uuid,
    /// The module it relayed for, when this was a relay. `None` when the caller
    /// reported for itself.
    pub relayed_for: Option<uuid::Uuid>,
}

/// Authorize a power report, which has two legitimate shapes.
///
/// **Direct.** A module with its own site credentials, bound to the tool, posts
/// its own reading. `authorize_toolguard`'s rule covers this unchanged.
///
/// **Relayed (#161).** A module behind an edge has no site credentials and should
/// not have any -- it speaks the local broker and nothing else. Its reading
/// reaches the server only because the edge forwards it, signed with the *edge's*
/// token. The edge is not bound to the tool, so the plain rule refused every such
/// report: `authorize_toolguard` requires the CALLER's binding, and the caller is
/// a courier rather than a party to it.
///
/// That refusal was silent in both directions. The edge spawns the forward and
/// only warns on a transport error, and a 401 is a successful HTTP call -- so
/// nothing was logged at either end, and the server's power aggregation, its
/// `relay_on` record and #84's bypass detector received nothing at all from any
/// module behind an edge. The lease path hid it: a lease is decided entirely
/// edge-locally, so every tool still worked.
///
/// So a relay is accepted when BOTH bindings exist, and both are an
/// administrator's statement rather than anything a caller asserts:
///
/// * the caller is bound to the tool in the **`edge`** role -- "this coordinator
///   speaks for this tool", the same binding a door already requires of its
///   coordinator; and
/// * the named `device_id` is bound to the same tool in the **`power`** role --
///   the module the reading is actually about.
///
/// Why not simply trust any registered device that names a correctly-bound
/// module: this data feeds circuit aggregation, which can trip a circuit, and the
/// bypass detector. Widening "may post power data for this tool" from *bound to
/// it* to *any device with a token* is not a trade worth making for an admin step
/// that the doors model already asks for. And gating on the device's **declared**
/// `edge` capability would be theatre -- capabilities are self-asserted at
/// registration, so any device can claim one.
///
/// What this does NOT add: trust. An edge already holds the card pepper and
/// authorizes swipes offline for parts too small to run argon2 (#109), so a device
/// that can say "this card may energize this machine" is not escalated by being
/// able to say "this plug drew four amps". The binding makes the existing trust
/// explicit and scoped, rather than conferring new trust.
async fn authorize_power_report(
    state: &AppState,
    headers: &HeaderMap,
    toolguard_id: &str,
    subject: Option<uuid::Uuid>,
) -> Result<PowerReportCaller, ApiError> {
    // The direct case, and the device-wide case (an empty tool_id, validated by
    // the caller straight after). Unchanged.
    match authorize_toolguard(state, headers, toolguard_id).await {
        Ok(device_id) => {
            return Ok(PowerReportCaller {
                device_id,
                relayed_for: None,
            })
        }
        // A server fault stays a server fault -- the same reasoning
        // `authorize_toolguard` documents. Only an authorization REFUSAL is worth
        // reconsidering as a relay; reporting an outage as "not bound to that
        // tool" would send whoever is holding a dead plug looking in the wrong
        // place. Asked of the error rather than matched against one variant, so
        // this agrees with the status table by construction and covers
        // `DatabaseError` too.
        Err(e) if e.is_server_error() => return Err(e),
        Err(_) => {}
    }

    // Past here the caller holds a valid token but is not bound to the tool. The
    // only remaining way through is as that tool's coordinator, relaying for one
    // of its modules.
    let (device_id, _) = extract_device_auth(state, headers).await?;

    let Some(subject) = subject else {
        tracing::warn!(
            "Rejected power-report from device {device_id} for tool_id={toolguard_id}: \
             not bound to it, and no device_id to relay for"
        );
        return Err(ApiError::Unauthorized(
            "This device is not bound to that tool".to_string(),
        ));
    };

    let tool = find_tool_by_toolguard_id(state, toolguard_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    let is_coordinator = state.db.device_is_bound_to_tool_in_role(
        device_id,
        tool.id,
        crate::models::binding_role::EDGE,
    )?;
    let subject_is_a_module = state.db.device_is_bound_to_tool_in_role(
        subject,
        tool.id,
        crate::models::binding_role::POWER,
    )?;

    if is_coordinator && subject_is_a_module {
        return Ok(PowerReportCaller {
            device_id,
            relayed_for: Some(subject),
        });
    }

    // Said separately, because the two are different operator mistakes and
    // "unauthorized" sends you looking in the wrong place for one of them.
    tracing::warn!(
        "Rejected power-report relay from device {device_id} for tool {} \
         (coordinator binding: {is_coordinator}, subject {subject} bound as power: \
         {subject_is_a_module})",
        tool.id
    );
    Err(ApiError::Unauthorized(if !is_coordinator {
        "This device is not bound to that tool as its edge coordinator".to_string()
    } else {
        "The device_id named is not bound to that tool in the power role".to_string()
    }))
}

/// A billable report must come from the thing that actually switches the tool: a
/// device bound to it in the `power` role.
///
/// #101 slice 6: this replaces `metered_key_ok`, which demanded the tool's own
/// `external_api_key` so that one leaked shared key could not post charges for
/// every tool. The binding gives the same per-tool scoping -- a device bound to
/// some *other* tool cannot post a charge here -- and it is the stronger of the
/// two at rest, because a device token is stored hashed (#120/#14) where
/// `external_api_key` was a plaintext column.
///
/// It is also narrower than entry auth on purpose. `authorize_toolguard` accepts
/// any binding, so a `reader` may start a tool; only the `power` binding may put
/// money on it.
async fn metered_device_ok(
    state: &AppState,
    device_id: uuid::Uuid,
    tool: &crate::models::Tool,
) -> Result<bool, ApiError> {
    state
        .db
        .device_is_bound_to_tool_in_role(device_id, tool.id, crate::models::binding_role::POWER)
        .map_err(ApiError::from)
}

async fn resolve_card(
    state: &AppState,
    card: &str,
) -> Result<crate::models::CardResolution, ApiError> {
    let config = state.config_manager.get_config();
    let profile_field = &config.toolguard.profile_field;
    state
        .db
        .resolve_card(profile_field, card, state.card_cipher.as_deref())
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
        "card_id": card.id,
        "card_status": card.status,
        "context": "tool",
    });
    let user_id = user.id;
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::RevokedCardPresented,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        )
        .await
    {
        tracing::error!("Failed to write audit event: {}", e);
    }
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
    let user_id = user.map(|u| u.id);
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::ToolAccessDenied,
            user_id,
            user_id,
            details,
            None,
            None,
        )
        .await
    {
        tracing::error!("Failed to write audit event: {}", e);
    }
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
    let user_id = user.id;
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::ToolActivated,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        )
        .await
    {
        tracing::error!("Failed to write audit event: {}", e);
    }
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
    let user_id = user.id;
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::ToolDeactivated,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        )
        .await
    {
        tracing::error!("Failed to write audit event: {}", e);
    }
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
    let user_id = user.id;
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::ToolUsageLogged,
            Some(user_id),
            Some(user_id),
            details,
            None,
            None,
        )
        .await
    {
        tracing::error!("Failed to write audit event: {}", e);
    }
    Ok(())
}
