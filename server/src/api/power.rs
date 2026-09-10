//! Physical power topology (#41 / #42): circuits, outlets, receptacles, and a
//! tool's receptacle assignment.
//!
//! Admin endpoints under `/api/admin/power` manage the topology. A circuit is a
//! breaker (voltage rating + amperage limit) that may nest under an upstream
//! trunk circuit (`parent_circuit_id`, cycle-guarded here exactly as
//! [`crate::api::places`] guards the place tree). An outlet belongs to one
//! circuit and one room (`place_id`); a receptacle is a socket on an outlet; a
//! tool plugs into zero or one receptacle.
//!
//! This is the model #43 aggregates draw over and #44 interrupts. Nothing here
//! energizes or de-energizes anything -- it is the map, not the switch.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use bigdecimal::BigDecimal;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::database::DatabaseError;
use crate::models::{
    AuditEventType, NewAuditLog, NewPowerCircuit, NewPowerOutlet, NewPowerReceptacle, PowerCircuit,
    PowerOutlet, PowerReceptacle, ToolPowerState, UpdatePowerCircuit, UpdatePowerOutlet,
    UpdatePowerReceptacle,
};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config))
        .route("/circuits", get(list_circuits).post(create_circuit))
        .route(
            "/circuits/{id}",
            get(get_circuit)
                .patch(update_circuit)
                .delete(delete_circuit),
        )
        .route("/outlets", get(list_outlets).post(create_outlet))
        .route(
            "/outlets/{id}",
            get(get_outlet).patch(update_outlet).delete(delete_outlet),
        )
        .route(
            "/receptacles",
            get(list_receptacles).post(create_receptacle),
        )
        .route(
            "/receptacles/{id}",
            get(get_receptacle)
                .patch(update_receptacle)
                .delete(delete_receptacle),
        )
        .route(
            "/tools/{tool_id}/receptacle",
            axum::routing::put(assign_tool_receptacle),
        )
        .route("/telemetry", get(get_telemetry))
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct PowerConfigResponse {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateCircuitRequest {
    pub breaker_label: String,
    pub voltage_rating: i32,
    pub amperage_limit: BigDecimal,
    #[serde(default)]
    pub parent_circuit_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateCircuitRequest {
    pub breaker_label: Option<String>,
    pub voltage_rating: Option<i32>,
    pub amperage_limit: Option<BigDecimal>,
    /// Outer `Some` = field present; inner `None` = promote to a top-level trunk.
    #[serde(default)]
    pub parent_circuit_id: Option<Option<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct CircuitDetailResponse {
    #[serde(flatten)]
    pub circuit: PowerCircuit,
    /// Ordered top-down: `[trunk, ..., immediate_parent]`.
    pub ancestors: Vec<PowerCircuit>,
    pub outlets: Vec<PowerOutlet>,
}

#[derive(Debug, Deserialize)]
pub struct CreateOutletRequest {
    pub circuit_id: Uuid,
    pub place_id: Uuid,
    pub label: String,
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateOutletRequest {
    pub circuit_id: Option<Uuid>,
    pub place_id: Option<Uuid>,
    pub label: Option<String>,
    #[serde(default)]
    pub location: Option<Option<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OutletQuery {
    pub circuit_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct OutletDetailResponse {
    #[serde(flatten)]
    pub outlet: PowerOutlet,
    pub receptacles: Vec<PowerReceptacle>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReceptacleRequest {
    pub outlet_id: Uuid,
    pub label: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateReceptacleRequest {
    pub outlet_id: Option<Uuid>,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReceptacleQuery {
    pub outlet_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AssignReceptacleRequest {
    /// The receptacle to plug this tool into, or `null` to unplug it.
    pub receptacle_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct CircuitDraw {
    pub circuit_id: Uuid,
    /// Summed latest draw (amps) over the tools on this circuit.
    pub total_draw_amps: BigDecimal,
}

#[derive(Debug, Serialize)]
pub struct PowerTelemetryResponse {
    /// One entry per circuit (0 when idle), so the UI can show draw against the
    /// circuit's `amperage_limit`.
    pub circuits: Vec<CircuitDraw>,
    /// Latest reading for every tool that has reported.
    pub tools: Vec<ToolPowerState>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_enabled(state: &AppState) -> Result<(), ApiError> {
    if !state.config_manager.get_config().power.enabled {
        return Err(ApiError::Forbidden(
            "Power topology module is disabled in server configuration".to_string(),
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
        tracing::error!("Failed to write power audit log {}: {}", event.as_str(), e);
    }
}

/// Map the two database errors a caller can provoke into their honest HTTP
/// codes with a message that says which constraint bit -- rather than a blanket
/// 500. Anything else keeps `ApiError::from`'s classification.
fn map_power_conflict(e: DatabaseError) -> ApiError {
    match e {
        DatabaseError::Diesel(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => ApiError::Conflict("That receptacle is already assigned to another tool".to_string()),
        DatabaseError::Diesel(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::ForeignKeyViolation,
            _,
        )) => ApiError::Conflict(
            "This item is still referenced (a sub-circuit, outlet, or receptacle); \
             move or delete those first"
                .to_string(),
        ),
        other => ApiError::from(other),
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

async fn get_config(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let cfg = state.config_manager.get_config();
    Ok(Json(ApiResponse::success(PowerConfigResponse {
        enabled: cfg.power.enabled,
    })))
}

// ---------------------------------------------------------------------------
// Circuits
// ---------------------------------------------------------------------------

async fn list_circuits(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let circuits = state.db.list_power_circuits()?;
    Ok(Json(ApiResponse::success(circuits)))
}

async fn get_circuit(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let circuit = state.db.get_power_circuit(id)?;
    let ancestors = state.db.power_circuit_ancestors(id)?;
    let outlets = state.db.list_outlets_for_circuit(id)?;
    Ok(Json(ApiResponse::success(CircuitDetailResponse {
        circuit,
        ancestors,
        outlets,
    })))
}

async fn create_circuit(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateCircuitRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if req.breaker_label.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "breaker_label is required".to_string(),
        ));
    }
    if req.voltage_rating <= 0 {
        return Err(ApiError::BadRequest(
            "voltage_rating must be positive".to_string(),
        ));
    }
    if req.amperage_limit <= BigDecimal::from(0) {
        return Err(ApiError::BadRequest(
            "amperage_limit must be positive".to_string(),
        ));
    }
    if let Some(parent) = req.parent_circuit_id {
        // Fail with 404 rather than a raw FK 409 when the trunk doesn't exist.
        state
            .db
            .get_power_circuit(parent)
            .map_err(|_| ApiError::BadRequest("parent_circuit_id does not exist".to_string()))?;
    }

    let created = state
        .db
        .create_power_circuit(&NewPowerCircuit {
            breaker_label: req.breaker_label.trim().to_string(),
            voltage_rating: req.voltage_rating,
            amperage_limit: req.amperage_limit,
            parent_circuit_id: req.parent_circuit_id,
        })
        .map_err(map_power_conflict)?;

    audit(
        &state,
        AuditEventType::PowerCircuitCreated,
        Some(admin.0.id),
        serde_json::json!({
            "circuit_id": created.id,
            "breaker_label": created.breaker_label,
            "voltage_rating": created.voltage_rating,
            "amperage_limit": created.amperage_limit.to_string(),
            "parent_circuit_id": created.parent_circuit_id,
        }),
    );

    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn update_circuit(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateCircuitRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let _before = state.db.get_power_circuit(id)?;

    if let Some(v) = req.voltage_rating {
        if v <= 0 {
            return Err(ApiError::BadRequest(
                "voltage_rating must be positive".to_string(),
            ));
        }
    }
    if let Some(a) = &req.amperage_limit {
        if a <= &BigDecimal::from(0) {
            return Err(ApiError::BadRequest(
                "amperage_limit must be positive".to_string(),
            ));
        }
    }

    // Self-parent + cycle guard, only when parent_circuit_id is being set to a
    // value. Mirrors the place-tree guard: walk the prospective parent's
    // ancestors and reject if this circuit appears.
    if let Some(Some(parent)) = req.parent_circuit_id {
        if parent == id {
            return Err(ApiError::BadRequest(
                "A circuit can't be its own upstream trunk".to_string(),
            ));
        }
        // Existence -> 400 rather than a raw FK 409.
        state
            .db
            .get_power_circuit(parent)
            .map_err(|_| ApiError::BadRequest("parent_circuit_id does not exist".to_string()))?;
        let chain = state.db.power_circuit_ancestors(parent)?;
        if chain.iter().any(|c| c.id == id) {
            return Err(ApiError::BadRequest(
                "Change would create a cycle in the circuit hierarchy".to_string(),
            ));
        }
    }

    let changes = UpdatePowerCircuit {
        breaker_label: req.breaker_label.map(|s| s.trim().to_string()),
        voltage_rating: req.voltage_rating,
        amperage_limit: req.amperage_limit,
        parent_circuit_id: req.parent_circuit_id,
        updated_at: Some(Utc::now()),
    };

    let after = state
        .db
        .update_power_circuit(id, &changes)
        .map_err(map_power_conflict)?;

    audit(
        &state,
        AuditEventType::PowerCircuitUpdated,
        Some(admin.0.id),
        serde_json::json!({ "circuit_id": id }),
    );

    Ok(Json(ApiResponse::success(after)))
}

async fn delete_circuit(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    // Refuse deletion while anything still hangs off this circuit, and say so
    // with a 409. The database enforces the same thing (ON DELETE RESTRICT on
    // both the outlet FK and the self-referential trunk FK), but diesel 2.1
    // reports a delete-side RESTRICT violation as `Kind::Unknown` rather than
    // `ForeignKeyViolation`, so the raw error would surface as a 500. Checking
    // here keeps the honest 409 and gives a message that names what is in the
    // way. The RESTRICT is still the real guard against a concurrent insert.
    let child_circuits = state
        .db
        .list_power_circuits()?
        .into_iter()
        .filter(|c| c.parent_circuit_id == Some(id))
        .count();
    let outlets = state.db.list_outlets_for_circuit(id)?.len();
    if child_circuits > 0 || outlets > 0 {
        return Err(ApiError::Conflict(
            "This circuit still has sub-circuits or outlets; move or delete them first".to_string(),
        ));
    }

    let deleted = state
        .db
        .delete_power_circuit(id)
        .map_err(map_power_conflict)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Circuit not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::PowerCircuitDeleted,
        Some(admin.0.id),
        serde_json::json!({ "circuit_id": id }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Circuit deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Outlets
// ---------------------------------------------------------------------------

async fn list_outlets(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<OutletQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let outlets = match q.circuit_id {
        Some(cid) => state.db.list_outlets_for_circuit(cid)?,
        None => state.db.list_power_outlets()?,
    };
    Ok(Json(ApiResponse::success(outlets)))
}

async fn get_outlet(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let outlet = state.db.get_power_outlet(id)?;
    let receptacles = state.db.list_receptacles_for_outlet(id)?;
    Ok(Json(ApiResponse::success(OutletDetailResponse {
        outlet,
        receptacles,
    })))
}

async fn create_outlet(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateOutletRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if req.label.trim().is_empty() {
        return Err(ApiError::BadRequest("label is required".to_string()));
    }
    // Existence checks -> 400 rather than a raw FK 409.
    state
        .db
        .get_power_circuit(req.circuit_id)
        .map_err(|_| ApiError::BadRequest("circuit_id does not exist".to_string()))?;
    state
        .db
        .get_place(req.place_id)
        .map_err(|_| ApiError::BadRequest("place_id does not exist".to_string()))?;

    let created = state
        .db
        .create_power_outlet(&NewPowerOutlet {
            circuit_id: req.circuit_id,
            place_id: req.place_id,
            label: req.label.trim().to_string(),
            location: req.location,
        })
        .map_err(map_power_conflict)?;

    audit(
        &state,
        AuditEventType::PowerOutletCreated,
        Some(admin.0.id),
        serde_json::json!({
            "outlet_id": created.id,
            "circuit_id": created.circuit_id,
            "place_id": created.place_id,
            "label": created.label,
        }),
    );

    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn update_outlet(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateOutletRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let _before = state.db.get_power_outlet(id)?;

    if let Some(cid) = req.circuit_id {
        state
            .db
            .get_power_circuit(cid)
            .map_err(|_| ApiError::BadRequest("circuit_id does not exist".to_string()))?;
    }
    if let Some(pid) = req.place_id {
        state
            .db
            .get_place(pid)
            .map_err(|_| ApiError::BadRequest("place_id does not exist".to_string()))?;
    }

    let changes = UpdatePowerOutlet {
        circuit_id: req.circuit_id,
        place_id: req.place_id,
        label: req.label.map(|s| s.trim().to_string()),
        location: req.location,
        updated_at: Some(Utc::now()),
    };

    let after = state
        .db
        .update_power_outlet(id, &changes)
        .map_err(map_power_conflict)?;

    audit(
        &state,
        AuditEventType::PowerOutletUpdated,
        Some(admin.0.id),
        serde_json::json!({ "outlet_id": id }),
    );

    Ok(Json(ApiResponse::success(after)))
}

async fn delete_outlet(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let deleted = state
        .db
        .delete_power_outlet(id)
        .map_err(map_power_conflict)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Outlet not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::PowerOutletDeleted,
        Some(admin.0.id),
        serde_json::json!({ "outlet_id": id }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Outlet deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Receptacles
// ---------------------------------------------------------------------------

async fn list_receptacles(
    State(state): State<AppState>,
    _admin: AdminUser,
    Query(q): Query<ReceptacleQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let receptacles = match q.outlet_id {
        Some(oid) => state.db.list_receptacles_for_outlet(oid)?,
        None => state.db.list_power_receptacles()?,
    };
    Ok(Json(ApiResponse::success(receptacles)))
}

async fn get_receptacle(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let receptacle = state.db.get_power_receptacle(id)?;
    Ok(Json(ApiResponse::success(receptacle)))
}

async fn create_receptacle(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateReceptacleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if req.label.trim().is_empty() {
        return Err(ApiError::BadRequest("label is required".to_string()));
    }
    state
        .db
        .get_power_outlet(req.outlet_id)
        .map_err(|_| ApiError::BadRequest("outlet_id does not exist".to_string()))?;

    let created = state
        .db
        .create_power_receptacle(&NewPowerReceptacle {
            outlet_id: req.outlet_id,
            label: req.label.trim().to_string(),
        })
        .map_err(map_power_conflict)?;

    audit(
        &state,
        AuditEventType::PowerReceptacleCreated,
        Some(admin.0.id),
        serde_json::json!({
            "receptacle_id": created.id,
            "outlet_id": created.outlet_id,
            "label": created.label,
        }),
    );

    Ok((StatusCode::CREATED, Json(ApiResponse::success(created))))
}

async fn update_receptacle(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateReceptacleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    let _before = state.db.get_power_receptacle(id)?;

    if let Some(oid) = req.outlet_id {
        state
            .db
            .get_power_outlet(oid)
            .map_err(|_| ApiError::BadRequest("outlet_id does not exist".to_string()))?;
    }

    let changes = UpdatePowerReceptacle {
        outlet_id: req.outlet_id,
        label: req.label.map(|s| s.trim().to_string()),
        updated_at: Some(Utc::now()),
    };

    let after = state
        .db
        .update_power_receptacle(id, &changes)
        .map_err(map_power_conflict)?;

    audit(
        &state,
        AuditEventType::PowerReceptacleUpdated,
        Some(admin.0.id),
        serde_json::json!({ "receptacle_id": id }),
    );

    Ok(Json(ApiResponse::success(after)))
}

async fn delete_receptacle(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let deleted = state
        .db
        .delete_power_receptacle(id)
        .map_err(map_power_conflict)?;
    if deleted == 0 {
        return Err(ApiError::NotFound("Receptacle not found".to_string()));
    }
    audit(
        &state,
        AuditEventType::PowerReceptacleDeleted,
        Some(admin.0.id),
        serde_json::json!({ "receptacle_id": id }),
    );
    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Receptacle deleted".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Tool <-> receptacle assignment
// ---------------------------------------------------------------------------

/// Plug a tool into a receptacle (or unplug it with `receptacle_id: null`). The
/// partial-unique index means claiming a receptacle another tool already holds
/// is a 409, not a silent double-binding.
async fn assign_tool_receptacle(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(tool_id): Path<Uuid>,
    Json(req): Json<AssignReceptacleRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_enabled(&state)?;
    if let Some(rid) = req.receptacle_id {
        state
            .db
            .get_power_receptacle(rid)
            .map_err(|_| ApiError::BadRequest("receptacle_id does not exist".to_string()))?;
    }

    let changed = state
        .db
        .assign_tool_receptacle(tool_id, req.receptacle_id)
        .map_err(map_power_conflict)?;
    if changed == 0 {
        return Err(ApiError::NotFound("Tool not found".to_string()));
    }

    audit(
        &state,
        AuditEventType::PowerReceptacleUpdated,
        Some(admin.0.id),
        serde_json::json!({ "tool_id": tool_id, "receptacle_id": req.receptacle_id }),
    );

    Ok(Json(ApiResponse::<()> {
        success: true,
        data: None,
        message: Some("Tool receptacle updated".to_string()),
        error: None,
    }))
}

// ---------------------------------------------------------------------------
// Telemetry (#43): latest draw per tool, aggregated per circuit
// ---------------------------------------------------------------------------

/// Latest reading per tool plus the per-circuit draw totals the live-draw UI
/// renders. The aggregation is done in Rust over a handful of small tables
/// rather than a SQL join: the relationship chain
/// (tool -> receptacle -> outlet -> circuit) is short, the row counts are tiny,
/// and reading the sum here is what lets #44's aggregation reuse the same shape.
async fn get_telemetry(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let states = state.db.list_tool_power_state()?;
    let links = state.db.all_tool_receptacle_links()?;
    let receptacles = state.db.list_power_receptacles()?;
    let outlets = state.db.list_power_outlets()?;
    let circuits = state.db.list_power_circuits()?;

    let recep_to_outlet: HashMap<Uuid, Uuid> =
        receptacles.iter().map(|r| (r.id, r.outlet_id)).collect();
    let outlet_to_circuit: HashMap<Uuid, Uuid> =
        outlets.iter().map(|o| (o.id, o.circuit_id)).collect();
    let tool_to_receptacle: HashMap<Uuid, Uuid> = links
        .into_iter()
        .filter_map(|(t, r)| r.map(|r| (t, r)))
        .collect();

    let mut totals: HashMap<Uuid, BigDecimal> = circuits
        .iter()
        .map(|c| (c.id, BigDecimal::from(0)))
        .collect();
    for s in &states {
        let Some(draw) = s.last_draw_amps.as_ref() else {
            continue;
        };
        let Some(rid) = tool_to_receptacle.get(&s.tool_id) else {
            continue;
        };
        let Some(oid) = recep_to_outlet.get(rid) else {
            continue;
        };
        let Some(cid) = outlet_to_circuit.get(oid) else {
            continue;
        };
        let entry = totals.entry(*cid).or_insert_with(|| BigDecimal::from(0));
        *entry = entry.clone() + draw.clone();
    }

    let mut circuit_draws: Vec<CircuitDraw> = totals
        .into_iter()
        .map(|(circuit_id, total_draw_amps)| CircuitDraw {
            circuit_id,
            total_draw_amps,
        })
        .collect();
    circuit_draws.sort_by(|a, b| a.circuit_id.cmp(&b.circuit_id));

    Ok(Json(ApiResponse::success(PowerTelemetryResponse {
        circuits: circuit_draws,
        tools: states,
    })))
}
