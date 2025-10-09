use axum::{
    extract::{Path, State, Query},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, NaiveDate, Utc};
use crate::{
    api::{
        errors::ApiError,
        responses::ApiResponse,
    },
    auth::{AuthUser, StaffUser},
    models::{
        trainers::{ToolTrainer, NewToolTrainer, UpdateToolTrainer, ToolTrainerWithUser,
                   TrainingRecord, NewTrainingRecord, UpdateTrainingRecord, TrainingRecordWithUsers,
                  TrainingCompletionStatus}
    },
    AppState,
};

// ==================== REQUEST/RESPONSE TYPES ====================

#[derive(Debug, Serialize, Deserialize)]
pub struct AssignTrainerRequest {
    pub user_id: Uuid,
    pub expires_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTrainerRequest {
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub is_active: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTrainingRecordRequest {
    pub tool_id: Uuid,
    pub training_step_id: Option<Uuid>, // Make it optional for backward compatibility
    pub trainee_user_id: Uuid,
    pub training_date: NaiveDate,
    pub completion_status: TrainingCompletionStatus,
    pub minutes_trained: Option<i32>,
    pub skills_covered: Option<Vec<String>>,
    pub notes: Option<String>,
    pub next_steps: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTrainingRecordRequest {
    pub completion_status: Option<TrainingCompletionStatus>,
    pub minutes_trained: Option<i32>,
    pub training_step_id: Option<Uuid>,
    pub skills_covered: Option<Vec<String>>,
    pub notes: Option<String>,
    pub next_steps: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainingRecordsQuery {
    pub tool_id: Option<Uuid>,
    pub trainer_id: Option<Uuid>,
    pub trainee_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ==================== ROUTER ====================

pub fn trainers_router() -> Router<AppState> {
    Router::new()
        // Tool trainer management (Staff only)
        .route("/tools/:tool_id/trainers", post(assign_tool_trainer).get(get_tool_trainers))
        .route("/tools/:tool_id/trainers/:user_id", put(update_tool_trainer).delete(remove_tool_trainer))
        
        // Training records (Trainers can create, all can view with restrictions)
        .route("/training-records", post(create_training_record).get(get_training_records))
        .route("/training-records/:record_id", put(update_training_record))
        .route("/users/:user_id/training-records", get(get_user_training_records))
        
        // Utility endpoints
        .route("/tools/:tool_id/trainers/check/:user_id", get(check_trainer_authorization))
}

// ==================== TOOL TRAINER MANAGEMENT ====================

/// Assign a user as a trainer for a specific tool (Staff only)
async fn assign_tool_trainer(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(tool_id): Path<Uuid>,
    Json(payload): Json<AssignTrainerRequest>,
) -> Result<Json<ApiResponse<ToolTrainer>>, ApiError> {
    // Verify the tool exists
    let _tool = state.db.get_tool_by_id(tool_id)
        .map_err(|e| {
            tracing::error!("Failed to get tool: {}", e);
            ApiError::InternalServerError("Failed to verify tool".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Tool not found".to_string()))?;
    // Verify the user exists
    let _user = state.db.find_user_by_id(payload.user_id)
        .map_err(|e| {
            tracing::error!("Failed to get user: {}", e);
            ApiError::InternalServerError("Failed to verify user".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

    let notes = payload.notes.clone();

    let new_trainer = NewToolTrainer {
        user_id: payload.user_id,
        tool_id,
        authorized_by: staff.0.id,
        expires_at: payload.expires_at,
        notes: payload.notes,
        is_active: Some(true),
    };

    let trainer = state.db.assign_tool_trainer(&new_trainer)
        .map_err(|e| {
            tracing::error!("Failed to assign trainer: {}", e);
            if e.to_string().contains("duplicate key") {
                ApiError::BadRequest("User is already assigned as trainer for this tool".to_string())
            } else {
                ApiError::InternalServerError("Failed to assign trainer".to_string())
            }
        })?;

    // Log the trainer assignment to audit logs
    if let Err(e) = state.audit_logger.log_event(
        crate::models::AuditEventType::TrainerAssigned,
        Some(payload.user_id),
        Some(staff.0.id),
        serde_json::json!({
            "tool_id": tool_id,
            "assigned_user_id": payload.user_id,
            "expires_at": payload.expires_at,
            "notes": notes
        }),
        Some(tool_id.to_string()),
        Some(format!("User assigned as trainer for tool")),
    ).await {
        tracing::warn!("Failed to log trainer assignment to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(trainer)))
}

/// Get all trainers for a specific tool
async fn get_tool_trainers(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(tool_id): Path<Uuid>,
    Query(query): Query<serde_json::Value>,
) -> Result<Json<ApiResponse<Vec<ToolTrainerWithUser>>>, ApiError> {
    let include_inactive = query.get("include_inactive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let trainers = state.db.get_tool_trainers(tool_id, include_inactive)
        .map_err(|e| {
            tracing::error!("Failed to get tool trainers: {}", e);
            ApiError::InternalServerError("Failed to retrieve tool trainers".to_string())
        })?;

    Ok(Json(ApiResponse::success(trainers)))
}

/// Update a tool trainer assignment (Staff only)
async fn update_tool_trainer(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path((tool_id, user_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<UpdateTrainerRequest>,
) -> Result<Json<ApiResponse<ToolTrainer>>, ApiError> {
    let update_trainer = UpdateToolTrainer {
        expires_at: payload.expires_at,
        is_active: payload.is_active,
        notes: payload.notes,
    };

    let updated_trainer = state.db.update_tool_trainer(tool_id, user_id, &update_trainer)
        .map_err(|e| {
            tracing::error!("Failed to update trainer: {}", e);
            ApiError::InternalServerError("Failed to update trainer".to_string())
        })?;

    Ok(Json(ApiResponse::success(updated_trainer)))
}

/// Remove a tool trainer assignment (Staff only)
async fn remove_tool_trainer(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path((tool_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    state.db.remove_tool_trainer(tool_id, user_id)
        .map_err(|e| {
            tracing::error!("Failed to remove trainer: {}", e);
            ApiError::InternalServerError("Failed to remove trainer".to_string())
        })?;

    // Log the trainer removal to audit logs
    if let Err(e) = state.audit_logger.log_event(
        crate::models::AuditEventType::TrainerRemoved,
        Some(user_id),
        Some(_staff.0.id),
        serde_json::json!({
            "tool_id": tool_id,
            "removed_user_id": user_id
        }),
        Some(tool_id.to_string()),
        Some(format!("User removed as trainer for tool")),
    ).await {
        tracing::warn!("Failed to log trainer removal to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(())))
}

/// Check if a user is authorized as a trainer for a specific tool
async fn check_trainer_authorization(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((tool_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<bool>>, ApiError> {
    let is_authorized = state.db.is_active_tool_trainer(tool_id, user_id)
        .map_err(|e| {
            tracing::error!("Failed to check trainer authorization: {}", e);
            ApiError::InternalServerError("Failed to check trainer authorization".to_string())
        })?;

    Ok(Json(ApiResponse::success(is_authorized)))
}

// ==================== TRAINING RECORDS ====================

/// Create a new training record (Trainers only)
async fn create_training_record(
    State(state): State<AppState>,
    user: AuthUser,
    Json(payload): Json<CreateTrainingRecordRequest>,
) -> Result<Json<ApiResponse<TrainingRecord>>, ApiError> {
    // Check if user is an active trainer for this tool
    let is_trainer = state.db.is_active_tool_trainer(payload.tool_id, user.0.id)
        .map_err(|e| {
            tracing::error!("Failed to check trainer status: {}", e);
            ApiError::InternalServerError("Failed to verify trainer status".to_string())
        })?;

    if !is_trainer {
        return Err(ApiError::Forbidden("User is not authorized as a trainer for this tool".to_string()));
    }

    let new_record = NewTrainingRecord {
        tool_id: payload.tool_id,
        training_step_id: payload.training_step_id,
        trainee_user_id: payload.trainee_user_id,
        trainer_user_id: user.0.id,
        training_date: payload.training_date,
        completion_status: payload.completion_status.to_string(),
        minutes_trained: payload.minutes_trained,
        skills_covered: payload.skills_covered.map(|v| v.into_iter().map(Some).collect()),
        notes: payload.notes,
        next_steps: payload.next_steps,
    };

    let record = state.db.create_training_record(&new_record)
        .map_err(|e| {
            tracing::error!("Failed to create training record: {}", e);
            ApiError::InternalServerError("Failed to create training record".to_string())
        })?;

    // Log the training completion to audit logs
    if let Err(e) = state.audit_logger.log_event(
        crate::models::AuditEventType::TrainingSessionCompleted,
        Some(new_record.trainee_user_id),
        Some(user.0.id),
        serde_json::json!({
            "tool_id": new_record.tool_id,
            "training_step_id": new_record.training_step_id,
            "completion_status": new_record.completion_status,
            "minutes_trained": new_record.minutes_trained,
            "training_date": new_record.training_date,
            "notes": new_record.notes
        }),
        Some(new_record.tool_id.to_string()),
        Some("Training session completed and recorded".to_string()),
    ).await {
        tracing::warn!("Failed to log training completion to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(record)))
}

/// Get training records with optional filters
async fn get_training_records(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<TrainingRecordsQuery>,
) -> Result<Json<ApiResponse<Vec<TrainingRecordWithUsers>>>, ApiError> {
    // Staff can view all records, regular users can only view their own records
    let (trainer_filter, trainee_filter) = if user.0.role.can_access_staff() {
        // Staff can use any filters
        (query.trainer_id, query.trainee_id)
    } else {
        // Regular users can only see records where they are trainer or trainee
        match (query.trainer_id, query.trainee_id) {
            (Some(trainer_id), Some(trainee_id)) => {
                if trainer_id == user.0.id || trainee_id == user.0.id {
                    (Some(trainer_id), Some(trainee_id))
                } else {
                    return Err(ApiError::Forbidden("Cannot view other users' training records".to_string()));
                }
            }
            (Some(trainer_id), None) => {
                if trainer_id == user.0.id {
                    (Some(trainer_id), None)
                } else {
                    return Err(ApiError::Forbidden("Cannot view other users' training records".to_string()));
                }
            }
            (None, Some(trainee_id)) => {
                if trainee_id == user.0.id {
                    (None, Some(trainee_id))
                } else {
                    return Err(ApiError::Forbidden("Cannot view other users' training records".to_string()));
                }
            }
            (None, None) => {
                // Default to showing records where user is involved
                (Some(user.0.id), Some(user.0.id))
            }
        }
    };

    let records = state.db.get_training_records(
        query.tool_id,
        trainer_filter,
        trainee_filter,
        query.limit,
        query.offset,
    ).map_err(|e| {
        tracing::error!("Failed to get training records: {}", e);
        ApiError::InternalServerError("Failed to retrieve training records".to_string())
    })?;

    Ok(Json(ApiResponse::success(records)))
}

/// Get training records for a specific user
async fn get_user_training_records(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user_id): Path<Uuid>,
    Query(query): Query<serde_json::Value>,
) -> Result<Json<ApiResponse<Vec<TrainingRecordWithUsers>>>, ApiError> {
    // Users can view their own records, staff can view anyone's
    if user.0.id != target_user_id && !user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden("Cannot view other users' training records".to_string()));
    }

    let as_trainer = query.get("as_trainer")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let records = state.db.get_user_training_records(target_user_id, as_trainer)
        .map_err(|e| {
            tracing::error!("Failed to get user training records: {}", e);
            ApiError::InternalServerError("Failed to retrieve training records".to_string())
        })?;

    Ok(Json(ApiResponse::success(records)))
}

/// Update a training record (Trainers can update their own records, staff can update any)
async fn update_training_record(
    State(state): State<AppState>,
    user: AuthUser,
    Path(record_id): Path<Uuid>,
    Json(payload): Json<UpdateTrainingRecordRequest>,
) -> Result<Json<ApiResponse<TrainingRecord>>, ApiError> {
    // Get the existing record to check permissions
    let existing_record = state.db.get_training_records(None, None, None, None, None)
        .map_err(|e| {
            tracing::error!("Failed to get training records: {}", e);
            ApiError::InternalServerError("Failed to verify record".to_string())
        })?
        .into_iter()
        .find(|r| r.record.id == record_id)
        .ok_or_else(|| ApiError::NotFound("Training record not found".to_string()))?;

    // Check permissions: trainers can update their own records, staff can update any
    if existing_record.record.trainer_user_id != user.0.id && !user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden("Cannot update other trainers' records".to_string()));
    }

    let update_record = UpdateTrainingRecord {
        completion_status: payload.completion_status.map(|s| s.to_string()),
        minutes_trained: payload.minutes_trained,
        training_step_id: payload.training_step_id,
        skills_covered: payload.skills_covered.map(|v| v.into_iter().map(Some).collect()),
        notes: payload.notes,
        next_steps: payload.next_steps,
    };

    let updated_record = state.db.update_training_record(record_id, &update_record)
        .map_err(|e| {
            tracing::error!("Failed to update training record: {}", e);
            ApiError::InternalServerError("Failed to update training record".to_string())
        })?;

    Ok(Json(ApiResponse::success(updated_record)))
}
