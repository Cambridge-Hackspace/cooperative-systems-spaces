use axum::{
    extract::{Path, State, Query},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::{
    api::{
        errors::ApiError,
        responses::ApiResponse,
    },
    auth::{AuthUser, StaffUser},
    models::{
        Tool, NewTool, ToolStatus, ToolCategory, ToolEvent, NewToolEvent, 
        ToolTrainingType, UserToolTraining, trainers::ToolTrainer
    },
    AppState,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateToolRequest {
    pub name: String,
    pub description: Option<String>,
    pub category: ToolCategory,
    pub barcode: Option<String>,
    pub serial_number: Option<String>,
    pub location: Option<String>,
    pub purchase_date: Option<chrono::NaiveDate>,
    pub purchase_price: Option<bigdecimal::BigDecimal>,
    pub maintenance_notes: Option<String>,
    pub requires_training: Option<bool>,
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, diesel::AsChangeset)]
#[diesel(table_name = crate::schema::tools, treat_none_as_null = false)]
pub struct UpdateToolRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<ToolCategory>,
    pub status: Option<ToolStatus>,
    pub barcode: Option<String>,
    pub serial_number: Option<String>,
    pub location: Option<String>,
    pub purchase_date: Option<chrono::NaiveDate>,
    pub purchase_price: Option<bigdecimal::BigDecimal>,
    pub maintenance_notes: Option<String>,
    pub requires_training: Option<bool>,
    pub external_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangeToolStatusRequest {
    pub status: ToolStatus,
    pub notes: Option<String>,
    pub scan_data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ToolQuery {
    pub category: Option<ToolCategory>,
    pub status: Option<ToolStatus>,
    pub requires_training: Option<bool>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

pub fn tools_routes() -> Router<AppState> {
    Router::new()
        // Tool CRUD operations (staff only)
        .route("/", get(list_tools).post(create_tool))
        .route("/:tool_id", get(get_tool).put(update_tool).delete(delete_tool))
        
        // Tool status and event management
        .route("/:tool_id/status", put(change_tool_status))
        .route("/:tool_id/events", get(get_tool_events).post(add_tool_event))
        
        // Tool training management
        .route("/:tool_id/training-types", get(get_tool_training_types).post(create_training_type))
        .route("/:tool_id/trainers", get(get_tool_trainers).post(authorize_trainer))
        
        // User training records
        .route("/:tool_id/user-training", get(get_user_training_for_tool))
        .route("/user-training", get(get_user_training))
        .route("/user-training/:training_id", post(complete_training).delete(revoke_training))
        
        // Public endpoints for members to view tools
        .route("/available", get(list_available_tools))
        .route("/:tool_id/can-use", get(can_user_use_tool))
}

/// List all tools with filtering (staff only)
async fn list_tools(
    _staff: StaffUser,
    State(state): State<AppState>,
    query: Query<ToolQuery>,
) -> Result<Json<ApiResponse<Vec<Tool>>>, ApiError> {
    let tools = state.db.get_tools(query.0)
        .map_err(|e| {
            tracing::error!("Failed to query tools: {}", e);
            ApiError::InternalServerError("Failed to fetch tools".to_string())
        })?;

    Ok(Json(ApiResponse::success(tools)))
}

/// Get a specific tool
async fn get_tool(
    _staff: StaffUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Tool>>, ApiError> {
    let tool = state.db.get_tool_by_id(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to query tool: {}", e);
            ApiError::InternalServerError("Failed to fetch tool".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    Ok(Json(ApiResponse::success(tool)))
}

/// Create a new tool (staff only)
async fn create_tool(
    staff: StaffUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateToolRequest>,
) -> Result<Json<ApiResponse<Tool>>, ApiError> {
    let new_tool = NewTool {
        name: payload.name,
        description: payload.description,
        category: payload.category,
        status: Some(ToolStatus::Idle),
        barcode: payload.barcode,
        serial_number: payload.serial_number,
        location: payload.location,
        purchase_date: payload.purchase_date,
        purchase_price: payload.purchase_price,
        maintenance_notes: payload.maintenance_notes,
        requires_training: payload.requires_training,
        created_by: staff.0.id,
        external_id: payload.external_id,
    };

    let created_tool = state.db.create_tool(&new_tool)
        .map_err(|e| {
            tracing::error!("Failed to create tool: {}", e);
            ApiError::InternalServerError("Failed to create tool".to_string())
        })?;

    // Log the tool creation event
    let event = NewToolEvent {
        tool_id: created_tool.id,
        event_type: "created".to_string(),
        old_status: None,
        new_status: Some(ToolStatus::Idle),
        user_id: Some(staff.0.id),
        actor_id: Some(staff.0.id),
        notes: Some("Tool created".to_string()),
        scan_data: None,
    };

    if let Err(e) = state.db.create_tool_event(&event) {
        tracing::warn!("Failed to log tool creation event: {}", e);
    }

    Ok(Json(ApiResponse::success_with_message(
        created_tool,
        "Tool created successfully".to_string(),
    )))
}

/// Update a tool (staff only)
async fn update_tool(
    _staff: StaffUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
    Json(payload): Json<UpdateToolRequest>,
) -> Result<Json<ApiResponse<Tool>>, ApiError> {
    let updated_tool = state.db.update_tool(tool_id, &payload)
        .map_err(|e| {
            tracing::error!("Failed to update tool: {}", e);
            ApiError::InternalServerError("Failed to update tool".to_string())
        })?;

    Ok(Json(ApiResponse::success_with_message(
        updated_tool,
        "Tool updated successfully".to_string(),
    )))
}

/// Delete a tool (staff only)
async fn delete_tool(
    staff: StaffUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    let tool = state.db.get_tool_by_id(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to query tool: {}", e);
            ApiError::InternalServerError("Database query failed".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    // Log the deletion event before deleting
    let event = NewToolEvent {
        tool_id,
        event_type: "deleted".to_string(),
        old_status: Some(tool.status.clone()),
        new_status: None,
        user_id: Some(staff.0.id),
        actor_id: Some(staff.0.id),
        notes: Some("Tool deleted".to_string()),
        scan_data: None,
    };

    if let Err(e) = state.db.create_tool_event(&event) {
        tracing::warn!("Failed to log tool deletion event: {}", e);
    }

    state.db.delete_tool(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to delete tool: {}", e);
            ApiError::InternalServerError("Failed to delete tool".to_string())
        })?;

    Ok(Json(ApiResponse::success_with_message(
        (),
        "Tool deleted successfully".to_string(),
    )))
}

/// Change tool status (staff only)
async fn change_tool_status(
    staff: StaffUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
    Json(payload): Json<ChangeToolStatusRequest>,
) -> Result<Json<ApiResponse<Tool>>, ApiError> {
    let old_tool = state.db.get_tool_by_id(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to query tool: {}", e);
            ApiError::InternalServerError("Database query failed".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    let updated_tool = state.db.update_tool_status(tool_id, &payload.status)
        .map_err(|e| {
            tracing::error!("Failed to update tool status: {}", e);
            ApiError::InternalServerError("Failed to update tool status".to_string())
        })?;

    // Log the status change event
    let event = NewToolEvent {
        tool_id,
        event_type: "status_change".to_string(),
        old_status: Some(old_tool.status),
        new_status: Some(payload.status),
        user_id: Some(staff.0.id),
        actor_id: Some(staff.0.id),
        notes: payload.notes,
        scan_data: payload.scan_data,
    };

    if let Err(e) = state.db.create_tool_event(&event) {
        tracing::warn!("Failed to log tool status change event: {}", e);
    }

    Ok(Json(ApiResponse::success_with_message(
        updated_tool,
        "Tool status updated successfully".to_string(),
    )))
}

/// Get tool events/history
async fn get_tool_events(
    _staff: StaffUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<ToolEvent>>>, ApiError> {
    let events = state.db.get_tool_events(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to query tool events: {}", e);
            ApiError::InternalServerError("Failed to fetch tool events".to_string())
        })?;

    Ok(Json(ApiResponse::success(events)))
}

/// Add a tool event (staff only)
async fn add_tool_event(
    staff: StaffUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
    Json(mut event): Json<NewToolEvent>,
) -> Result<Json<ApiResponse<ToolEvent>>, ApiError> {
    event.tool_id = tool_id;
    event.actor_id = Some(staff.0.id);

    let created_event = state.db.create_tool_event(&event)
        .map_err(|e| {
            tracing::error!("Failed to create tool event: {}", e);
            ApiError::InternalServerError("Failed to create tool event".to_string())
        })?;

    Ok(Json(ApiResponse::success(created_event)))
}

/// List available tools for members
async fn list_available_tools(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Tool>>>, ApiError> {
    let query = ToolQuery {
        category: None,
        status: Some(ToolStatus::Idle),
        requires_training: None,
        page: None,
        per_page: None,
    };

    let tools = state.db.get_tools(query)
        .map_err(|e| {
            tracing::error!("Failed to query available tools: {}", e);
            ApiError::InternalServerError("Failed to fetch tools".to_string())
        })?;

    Ok(Json(ApiResponse::success(tools)))
}

/// Check if user can use a specific tool
async fn can_user_use_tool(
    user: AuthUser,
    State(state): State<AppState>,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<bool>>, ApiError> {
    let tool = state.db.get_tool_by_id(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to query tool: {}", e);
            ApiError::InternalServerError("Database query failed".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;

    let can_use = if !tool.requires_training {
        true
    } else {
        state.db.user_has_valid_training(user.0.id, tool_id)
            .map_err(|e| {
                tracing::error!("Failed to check user training: {}", e);
                ApiError::InternalServerError("Failed to check training status".to_string())
            })?
    };

    Ok(Json(ApiResponse::success(can_use)))
}

/// Placeholder implementations for training-related endpoints
async fn get_tool_training_types(
    _staff: StaffUser,
    State(_state): State<AppState>,
    Path(_tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<ToolTrainingType>>>, ApiError> {
    // TODO: Implement training types retrieval
    Ok(Json(ApiResponse::success(vec![])))
}

async fn create_training_type(
    _staff: StaffUser,
    State(_state): State<AppState>,
    Path(_tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ToolTrainingType>>, ApiError> {
    // TODO: Implement training type creation
    Err(ApiError::NotImplemented("Training type creation not yet implemented".to_string()))
}

async fn get_tool_trainers(
    _staff: StaffUser,
    State(_state): State<AppState>,
    Path(_tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<ToolTrainer>>>, ApiError> {
    // TODO: Implement trainers retrieval
    Ok(Json(ApiResponse::success(vec![])))
}

async fn authorize_trainer(
    _staff: StaffUser,
    State(_state): State<AppState>,
    Path(_tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ToolTrainer>>, ApiError> {
    // TODO: Implement trainer authorization
    Err(ApiError::NotImplemented("Trainer authorization not yet implemented".to_string()))
}

async fn get_user_training_for_tool(
    _user: AuthUser,
    State(_state): State<AppState>,
    Path(_tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<UserToolTraining>>>, ApiError> {
    // TODO: Implement user training for tool retrieval
    Ok(Json(ApiResponse::success(vec![])))
}

async fn get_user_training(
    _user: AuthUser,
    State(_state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<UserToolTraining>>>, ApiError> {
    // TODO: Implement user training retrieval
    Ok(Json(ApiResponse::success(vec![])))
}

async fn complete_training(
    _staff: StaffUser,
    State(_state): State<AppState>,
    Path(_training_id): Path<Uuid>,
) -> Result<Json<ApiResponse<UserToolTraining>>, ApiError> {
    // TODO: Implement training completion
    Err(ApiError::NotImplemented("Training completion not yet implemented".to_string()))
}

async fn revoke_training(
    _staff: StaffUser,
    State(_state): State<AppState>,
    Path(_training_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    // TODO: Implement training revocation
    Err(ApiError::NotImplemented("Training revocation not yet implemented".to_string()))
}
