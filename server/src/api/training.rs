use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::{AuthUser, StaffUser},
    models::{
        AssessmentType, CompleteTrainingRequest, NewTrainingInstructor, NewTrainingPrerequisite,
        NewTrainingStep, StartTrainingRequest, ToolTrainingOverview, TrainingInstructor,
        TrainingPrerequisite, TrainingStatus, TrainingStep, TrainingStepWithProgress,
        UpdateTrainingStep, UpdateUserTrainingProgress, User, UserTrainingProgress,
    },
    AppState,
};

// ==================== REQUEST/RESPONSE TYPES ====================

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTrainingStepRequest {
    pub tool_id: Uuid,
    pub step_number: i32,
    pub step_name: String,
    pub description: Option<String>,
    pub training_materials_url: Option<String>,
    pub requires_assessment: Option<bool>,
    pub assessment_type: Option<AssessmentType>,
    pub duration_minutes: Option<i32>,
    pub expires_after_days: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTrainingStepRequest {
    pub step_name: Option<String>,
    pub description: Option<String>,
    pub training_materials_url: Option<String>,
    pub requires_assessment: Option<bool>,
    pub assessment_type: Option<AssessmentType>,
    pub duration_minutes: Option<i32>,
    pub expires_after_days: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainingQuery {
    pub tool_id: Option<Uuid>,
    pub status: Option<TrainingStatus>,
    pub user_id: Option<Uuid>,
    pub instructor_id: Option<Uuid>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CertifyInstructorRequest {
    pub user_id: Uuid,
    pub training_step_id: Uuid,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateProgressRequest {
    pub status: TrainingStatus,
    pub assessment_score: Option<i32>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainingHistoryQuery {
    pub trainee_id: Option<Uuid>,
    pub trainer_id: Option<Uuid>,
    pub step_id: Option<Uuid>,
    pub completion_status: Option<String>,
    pub start_date: Option<chrono::NaiveDate>,
    pub end_date: Option<chrono::NaiveDate>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainingHistoryRecord {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub tool_name: String,
    pub training_step_id: Uuid,
    pub step_name: String,
    pub step_number: i32,
    pub trainee_user_id: Uuid,
    pub trainee_name: String,
    pub trainee_email: String,
    pub trainer_user_id: Uuid,
    pub trainer_name: String,
    pub trainer_email: String,
    pub training_date: chrono::NaiveDate,
    pub completion_status: String,
    pub minutes_trained: Option<i32>,
    pub notes: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

// ==================== ROUTER ====================

pub fn training_router() -> Router<AppState> {
    Router::new()
        // Training Roster - Users available for training (Trainers and Staff)
        .route("/roster", get(get_training_roster))
        .route("/roster/{tool_id}", get(get_training_roster_for_tool))
        // Training History - Records of completed training (Trainers and Staff)
        .route("/history/{tool_id}", get(get_training_history_for_tool))
        // Training Steps Management (Staff only)
        .route("/steps", post(create_training_step).get(get_training_steps))
        .route(
            "/steps/{step_id}",
            get(get_training_step)
                .put(update_training_step)
                .delete(delete_training_step),
        )
        .route(
            "/steps/{step_id}/position",
            put(update_training_step_position),
        )
        // Prerequisites Management (Staff only)
        .route(
            "/steps/{step_id}/prerequisites",
            post(add_prerequisite).get(get_prerequisites),
        )
        .route("/prerequisites/{prereq_id}", delete(remove_prerequisite))
        // Tool Training Overview (All authenticated users)
        .route("/tools/{tool_id}/overview", get(get_tool_training_overview))
        .route(
            "/tools/{tool_id}/overview/me",
            get(get_my_tool_training_overview),
        )
        .route(
            "/tools/{tool_id}/overview/{user_id}",
            get(get_user_tool_training_overview),
        )
        .route("/tools/{tool_id}/steps", get(get_tool_training_steps))
        // User Training Progress (All authenticated users for self, staff for others)
        .route("/progress", get(get_user_training_progress))
        .route(
            "/progress/{user_id}",
            get(get_user_training_progress_by_user),
        )
        .route(
            "/progress/{user_id}/{step_id}",
            get(get_specific_progress).put(update_training_progress),
        )
        // Training Session Management
        .route("/sessions/start", post(start_training_session))
        .route("/sessions/complete", post(complete_training_session))
        // Instructor Certification (Staff only)
        .route(
            "/instructors",
            post(certify_instructor).get(get_instructors),
        )
        .route(
            "/instructors/{instructor_id}",
            delete(revoke_instructor_certification),
        )
        // Tool Access Validation
        .route("/access/{tool_id}/{user_id}", get(check_tool_access))
        .route("/access/{tool_id}", get(check_my_tool_access))
}

// ==================== TRAINING ROSTER ====================

/// Get all active users available for training (Trainers and Staff)
async fn get_training_roster(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<ApiResponse<Vec<User>>>, ApiError> {
    // Check if user is either staff or a trainer for any tool
    if !user.0.role.can_access_staff() && !is_user_a_trainer(&state, user.0.id).await? {
        return Err(ApiError::Forbidden(
            "Must be a trainer or staff to access training roster".to_string(),
        ));
    }

    // Get all active users
    let users = state.db.get_all_active_users().map_err(|e| {
        tracing::error!("Failed to get training roster: {}", e);
        ApiError::InternalServerError("Failed to retrieve training roster".to_string())
    })?;

    Ok(Json(ApiResponse::success(users)))
}

/// Get users available for training on a specific tool (Trainers for that tool and Staff)
async fn get_training_roster_for_tool(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<User>>>, ApiError> {
    // Check if user is either staff or a trainer for this specific tool
    let is_trainer_for_tool = state
        .db
        .is_user_trainer_for_tool(user.0.id, tool_id)
        .map_err(|e| {
            tracing::error!("Failed to check trainer status: {}", e);
            ApiError::InternalServerError("Failed to verify trainer permissions".to_string())
        })?;

    if !user.0.role.can_access_staff() && !is_trainer_for_tool {
        return Err(ApiError::Forbidden(
            "Must be a trainer for this tool or staff to access tool training roster".to_string(),
        ));
    }

    // Get all active users - for now, all trainers can see all users
    // In the future, this could be filtered based on:
    // - User class requirements for the tool
    // - Training prerequisites
    // - Age restrictions
    // - Certification requirements, etc.
    let users = state.db.get_all_active_users().map_err(|e| {
        tracing::error!("Failed to get tool training roster: {}", e);
        ApiError::InternalServerError("Failed to retrieve tool training roster".to_string())
    })?;

    Ok(Json(ApiResponse::success(users)))
}

/// Helper function to check if user is a trainer for any tool
async fn is_user_a_trainer(state: &AppState, user_id: Uuid) -> Result<bool, ApiError> {
    state.db.is_user_trainer_for_any_tool(user_id).map_err(|e| {
        tracing::error!("Failed to check if user is trainer: {}", e);
        ApiError::InternalServerError("Failed to verify trainer status".to_string())
    })
}

// ==================== TRAINING HISTORY ====================

/// Get training history for a specific tool (Trainers for that tool and Staff)
async fn get_training_history_for_tool(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tool_id): Path<Uuid>,
    Query(query): Query<TrainingHistoryQuery>,
) -> Result<Json<ApiResponse<Vec<TrainingHistoryRecord>>>, ApiError> {
    // Check if user is either staff or a trainer for this specific tool
    let is_trainer_for_tool = state
        .db
        .is_user_trainer_for_tool(user.0.id, tool_id)
        .map_err(|e| {
            tracing::error!("Failed to check trainer status: {}", e);
            ApiError::InternalServerError("Failed to verify trainer permissions".to_string())
        })?;

    if !user.0.role.can_access_staff() && !is_trainer_for_tool {
        return Err(ApiError::Forbidden(
            "Must be a trainer for this tool or staff to access training history".to_string(),
        ));
    }

    // Get training history for this tool
    let history = state
        .db
        .get_training_history_for_tool(tool_id, &query)
        .map_err(|e| {
            tracing::error!("Failed to get training history: {}", e);
            ApiError::InternalServerError("Failed to retrieve training history".to_string())
        })?;

    Ok(Json(ApiResponse::success(history)))
}

// ==================== TRAINING STEPS MANAGEMENT ====================

/// Create a new training step (Staff only)
async fn create_training_step(
    State(state): State<AppState>,
    _staff: StaffUser,
    Json(payload): Json<CreateTrainingStepRequest>,
) -> Result<Json<ApiResponse<TrainingStep>>, ApiError> {
    let new_step = NewTrainingStep {
        tool_id: payload.tool_id,
        step_number: payload.step_number,
        step_name: payload.step_name.clone(),
        description: payload.description.clone(),
        training_materials_url: payload.training_materials_url,
        requires_assessment: payload.requires_assessment,
        assessment_type: payload.assessment_type,
        duration_minutes: payload.duration_minutes,
        expires_after_days: payload.expires_after_days,
        created_by: _staff.0.id,
    };

    let step_name = payload.step_name.clone();
    let description = payload.description.clone();

    let step = state.db.create_training_step(&new_step).map_err(|e| {
        tracing::error!("Failed to create training step: {}", e);
        ApiError::InternalServerError("Failed to create training step".to_string())
    })?;

    // Log the training step creation to audit logs
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::TrainingStepCreated,
            None,
            Some(_staff.0.id),
            serde_json::json!({
                "tool_id": payload.tool_id,
                "step_id": step.id,
                "step_number": payload.step_number,
                "step_name": step_name,
                "description": description,
                "requires_assessment": payload.requires_assessment
            }),
            Some(payload.tool_id.to_string()),
            Some(format!("Training step '{}' created", step_name)),
        )
        .await
    {
        tracing::warn!("Failed to log training step creation to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(step)))
}

/// Get all training steps with optional filtering
async fn get_training_steps(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<TrainingQuery>,
) -> Result<Json<ApiResponse<Vec<TrainingStep>>>, ApiError> {
    let steps = state.db.get_training_steps(&query).map_err(|e| {
        tracing::error!("Failed to get training steps: {}", e);
        ApiError::InternalServerError("Failed to retrieve training steps".to_string())
    })?;

    Ok(Json(ApiResponse::success(steps)))
}

/// Get a specific training step
async fn get_training_step(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(step_id): Path<Uuid>,
) -> Result<Json<ApiResponse<TrainingStep>>, ApiError> {
    let step = state
        .db
        .get_training_step_by_id(step_id)
        .map_err(|e| {
            tracing::error!("Failed to get training step: {}", e);
            ApiError::InternalServerError("Failed to retrieve training step".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Training step not found".to_string()))?;

    Ok(Json(ApiResponse::success(step)))
}

/// Update a training step (Staff only)
async fn update_training_step(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(step_id): Path<Uuid>,
    Json(payload): Json<UpdateTrainingStepRequest>,
) -> Result<Json<ApiResponse<TrainingStep>>, ApiError> {
    let update_step = UpdateTrainingStep {
        step_name: payload.step_name,
        description: payload.description,
        training_materials_url: payload.training_materials_url,
        requires_assessment: payload.requires_assessment,
        assessment_type: payload.assessment_type,
        duration_minutes: payload.duration_minutes,
        expires_after_days: payload.expires_after_days,
    };

    let updated_step = state
        .db
        .update_training_step(step_id, &update_step)
        .map_err(|e| {
            tracing::error!("Failed to update training step: {}", e);
            ApiError::InternalServerError("Failed to update training step".to_string())
        })?;

    Ok(Json(ApiResponse::success(updated_step)))
}

/// Delete a training step (Staff only)
async fn delete_training_step(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(step_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    // Check if any users have progress on this step
    let has_progress = state.db.has_user_progress_for_step(step_id).map_err(|e| {
        tracing::error!("Failed to check training step usage: {}", e);
        ApiError::InternalServerError("Failed to check training step usage".to_string())
    })?;

    if has_progress {
        return Err(ApiError::BadRequest(
            "Cannot delete training step with existing user progress".to_string(),
        ));
    }

    // Get step info for audit log before deletion
    let step = state
        .db
        .get_training_step_by_id(step_id)
        .map_err(|e| {
            tracing::error!("Failed to get training step: {}", e);
            ApiError::InternalServerError("Failed to retrieve training step".to_string())
        })?
        .ok_or_else(|| ApiError::NotFound("Training step not found".to_string()))?;

    state.db.delete_training_step(step_id).map_err(|e| {
        tracing::error!("Failed to delete training step: {}", e);
        ApiError::InternalServerError("Failed to delete training step".to_string())
    })?;

    // Log the training step deletion to audit logs
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::TrainingStepDeleted,
            None,
            Some(_staff.0.id),
            serde_json::json!({
                "tool_id": step.tool_id,
                "step_id": step_id,
                "step_number": step.step_number,
                "step_name": step.step_name,
                "description": step.description
            }),
            Some(step.tool_id.to_string()),
            Some(format!("Training step '{}' deleted", step.step_name)),
        )
        .await
    {
        tracing::warn!("Failed to log training step deletion to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(())))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateStepPositionRequest {
    pub step_number: i32,
}

/// Update a training step's position/order (Staff only)
async fn update_training_step_position(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(step_id): Path<Uuid>,
    Json(payload): Json<UpdateStepPositionRequest>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    state
        .db
        .update_training_step_position(step_id, payload.step_number)
        .map_err(|e| {
            tracing::error!("Failed to update training step position: {}", e);
            ApiError::InternalServerError("Failed to update training step position".to_string())
        })?;

    Ok(Json(ApiResponse::success(())))
}

// ==================== PREREQUISITES MANAGEMENT ====================

/// Add a prerequisite to a training step (Staff only)
async fn add_prerequisite(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(step_id): Path<Uuid>,
    Json(prerequisite_step_id): Json<Uuid>,
) -> Result<Json<ApiResponse<TrainingPrerequisite>>, ApiError> {
    let new_prereq = NewTrainingPrerequisite {
        training_step_id: step_id,
        prerequisite_step_id,
    };

    let prerequisite = state
        .db
        .add_training_prerequisite(&new_prereq)
        .map_err(|e| {
            tracing::error!("Failed to add prerequisite: {}", e);
            ApiError::InternalServerError("Failed to add prerequisite".to_string())
        })?;

    Ok(Json(ApiResponse::success(prerequisite)))
}

/// Get prerequisites for a training step
async fn get_prerequisites(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(step_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<TrainingStep>>>, ApiError> {
    let prerequisites = state.db.get_training_prerequisites(step_id).map_err(|e| {
        tracing::error!("Failed to get prerequisites: {}", e);
        ApiError::InternalServerError("Failed to retrieve prerequisites".to_string())
    })?;

    Ok(Json(ApiResponse::success(prerequisites)))
}

/// Remove a prerequisite (Staff only)
async fn remove_prerequisite(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(prereq_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    state
        .db
        .remove_training_prerequisite(prereq_id)
        .map_err(|e| {
            tracing::error!("Failed to remove prerequisite: {}", e);
            ApiError::InternalServerError("Failed to remove prerequisite".to_string())
        })?;

    Ok(Json(ApiResponse::success(())))
}

// ==================== TOOL TRAINING OVERVIEW ====================

/// Get comprehensive training overview for a tool (current user)
async fn get_tool_training_overview(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ToolTrainingOverview>>, ApiError> {
    let overview = state
        .db
        .get_tool_training_overview(tool_id, user.0.id)
        .map_err(|e| {
            // Logged, then converted. A blanket 500 here told the caller the
            // server broke when the row they named simply does not exist.
            tracing::warn!("get_tool_training_overview({tool_id}) failed: {e}");
            ApiError::from(e)
        })?;

    Ok(Json(ApiResponse::success(overview)))
}

/// Get comprehensive training overview for a tool for the current user (explicit /me route)
async fn get_my_tool_training_overview(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ToolTrainingOverview>>, ApiError> {
    let overview = state
        .db
        .get_tool_training_overview(tool_id, user.0.id)
        .map_err(|e| {
            // Logged, then converted. A blanket 500 here told the caller the
            // server broke when the row they named simply does not exist.
            tracing::warn!("get_tool_training_overview({tool_id}) failed: {e}");
            ApiError::from(e)
        })?;

    Ok(Json(ApiResponse::success(overview)))
}

/// Get comprehensive training overview for a tool for a specific user
async fn get_user_tool_training_overview(
    State(state): State<AppState>,
    user: AuthUser,
    Path((tool_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<ToolTrainingOverview>>, ApiError> {
    // Users can view their own overview, staff can view anyone's
    if user.0.id != target_user_id && !user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden(
            "Cannot view other users' training overview".to_string(),
        ));
    }

    let overview = state
        .db
        .get_tool_training_overview(tool_id, target_user_id)
        .map_err(|e| {
            // Logged, then converted. A blanket 500 here told the caller the
            // server broke when the row they named simply does not exist.
            tracing::warn!("get_tool_training_overview({tool_id}) failed: {e}");
            ApiError::from(e)
        })?;

    Ok(Json(ApiResponse::success(overview)))
}

/// Get training steps for a tool with user progress
async fn get_tool_training_steps(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<TrainingStepWithProgress>>>, ApiError> {
    let steps = state
        .db
        .get_tool_training_steps_with_progress(tool_id, user.0.id)
        .map_err(|e| {
            tracing::error!("Failed to get training steps: {}", e);
            ApiError::InternalServerError("Failed to retrieve training steps".to_string())
        })?;

    Ok(Json(ApiResponse::success(steps)))
}

// ==================== TRAINING PROGRESS ====================

/// Get user's training progress across all tools
async fn get_user_training_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<TrainingQuery>,
) -> Result<Json<ApiResponse<Vec<UserTrainingProgress>>>, ApiError> {
    let progress = state
        .db
        .get_user_training_progress(user.0.id, &query)
        .map_err(|e| {
            tracing::error!("Failed to get training progress: {}", e);
            ApiError::InternalServerError("Failed to retrieve training progress".to_string())
        })?;

    Ok(Json(ApiResponse::success(progress)))
}

/// Get training progress for a specific user (Staff only, or own progress)
async fn get_user_training_progress_by_user(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user_id): Path<Uuid>,
    Query(query): Query<TrainingQuery>,
) -> Result<Json<ApiResponse<Vec<UserTrainingProgress>>>, ApiError> {
    // Users can view their own progress, staff can view anyone's
    if user.0.id != target_user_id && !user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden(
            "Cannot view other users' training progress".to_string(),
        ));
    }

    let progress = state
        .db
        .get_user_training_progress(target_user_id, &query)
        .map_err(|e| {
            tracing::error!("Failed to get training progress: {}", e);
            ApiError::InternalServerError("Failed to retrieve training progress".to_string())
        })?;

    Ok(Json(ApiResponse::success(progress)))
}

/// Get specific training progress record
async fn get_specific_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((target_user_id, step_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<Option<UserTrainingProgress>>>, ApiError> {
    // Users can view their own progress, staff can view anyone's
    if user.0.id != target_user_id && !user.0.role.can_access_staff() {
        return Err(ApiError::Forbidden(
            "Cannot view other users' training progress".to_string(),
        ));
    }

    let progress = state
        .db
        .get_user_training_progress_for_step(target_user_id, step_id)
        .map_err(|e| {
            tracing::error!("Failed to get training progress: {}", e);
            ApiError::InternalServerError("Failed to retrieve training progress".to_string())
        })?;

    Ok(Json(ApiResponse::success(progress)))
}

/// Update training progress (Staff only, or instructors for their sessions)
async fn update_training_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((target_user_id, step_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<UpdateProgressRequest>,
) -> Result<Json<ApiResponse<UserTrainingProgress>>, ApiError> {
    // Check if user can update this progress
    let can_update = user.0.role.can_access_staff()
        || state
            .db
            .is_certified_instructor(user.0.id, step_id)
            .map_err(|e| {
                tracing::error!("Failed to check instructor status: {}", e);
                ApiError::InternalServerError("Failed to verify permissions".to_string())
            })?;

    if !can_update {
        return Err(ApiError::Forbidden(
            "Cannot update training progress".to_string(),
        ));
    }

    let update_progress = UpdateUserTrainingProgress {
        status: Some(payload.status),
        assessment_score: payload.assessment_score,
        notes: payload.notes,
        ..Default::default()
    };

    let updated_progress = state
        .db
        .update_user_training_progress(target_user_id, step_id, &update_progress)
        .map_err(|e| {
            tracing::error!("Failed to update training progress: {}", e);
            ApiError::InternalServerError("Failed to update training progress".to_string())
        })?;

    Ok(Json(ApiResponse::success(updated_progress)))
}

// ==================== TRAINING SESSIONS ====================

/// Start a training session
async fn start_training_session(
    State(state): State<AppState>,
    user: AuthUser,
    Json(payload): Json<StartTrainingRequest>,
) -> Result<Json<ApiResponse<UserTrainingProgress>>, ApiError> {
    // Prerequisites removed - all training steps are available by default
    // Users can start any training step without prerequisite validation

    let progress = state
        .db
        .start_training_session(user.0.id, &payload)
        .map_err(|e| {
            tracing::error!("Failed to start training session: {}", e);
            ApiError::InternalServerError("Failed to start training session".to_string())
        })?;

    // Log the training session start to audit logs
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::TrainingSessionStarted,
            Some(user.0.id),
            Some(user.0.id),
            serde_json::json!({
                "training_step_id": payload.training_step_id,
                "progress_id": progress.id,
                "started_by_user": user.0.id
            }),
            None,
            Some(format!("Training session started")),
        )
        .await
    {
        tracing::warn!("Failed to log training session start to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(progress)))
}

/// Complete a training session
async fn complete_training_session(
    State(state): State<AppState>,
    user: AuthUser,
    Json(payload): Json<CompleteTrainingRequest>,
) -> Result<Json<ApiResponse<UserTrainingProgress>>, ApiError> {
    // Validate that user can complete this training
    let can_complete = user.0.role.can_access_staff()
        || state
            .db
            .is_certified_instructor(user.0.id, payload.training_step_id)
            .map_err(|e| {
                tracing::error!("Failed to check instructor status: {}", e);
                ApiError::InternalServerError("Failed to verify permissions".to_string())
            })?;

    if !can_complete {
        return Err(ApiError::Forbidden(
            "Cannot complete training session".to_string(),
        ));
    }

    let progress = state
        .db
        .complete_training_session(user.0.id, &payload)
        .map_err(|e| {
            tracing::error!("Failed to complete training session: {}", e);
            ApiError::InternalServerError("Failed to complete training session".to_string())
        })?;

    // Log the training session completion to audit logs
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::TrainingSessionCompleted,
            None, // No specific user_id for this event type
            Some(user.0.id),
            serde_json::json!({
                "training_step_id": payload.training_step_id,
                "instructor_id": user.0.id,
                "assessment_score": payload.assessment_score,
                "passed": payload.passed,
                "notes": payload.notes.clone()
            }),
            None,
            Some(format!(
                "Training session completed with status: {:?}",
                payload.passed
            )),
        )
        .await
    {
        tracing::warn!("Failed to log training session completion to audit: {}", e);
    }

    // Broadcast updated toolguard state to all devices
    crate::api::toolguard::broadcast_toolguard_state(&state).await;

    Ok(Json(ApiResponse::success(progress)))
}

// ==================== INSTRUCTOR CERTIFICATION ====================

/// Certify a user as instructor for a training step (Staff only)
async fn certify_instructor(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(payload): Json<CertifyInstructorRequest>,
) -> Result<Json<ApiResponse<TrainingInstructor>>, ApiError> {
    let notes = payload.notes.clone();

    let new_instructor = NewTrainingInstructor {
        user_id: payload.user_id,
        training_step_id: payload.training_step_id,
        certified_by: staff.0.id,
        expires_at: payload.expires_at,
        notes: payload.notes,
    };

    let instructor = state.db.certify_instructor(&new_instructor).map_err(|e| {
        tracing::error!("Failed to certify instructor: {}", e);
        ApiError::InternalServerError("Failed to certify instructor".to_string())
    })?;

    // Log the instructor certification to audit logs
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::InstructorCertified,
            Some(payload.user_id),
            Some(staff.0.id),
            serde_json::json!({
                "training_step_id": payload.training_step_id,
                "certified_user_id": payload.user_id,
                "expires_at": payload.expires_at,
                "notes": notes
            }),
            None,
            Some(format!("User certified as instructor")),
        )
        .await
    {
        tracing::warn!("Failed to log instructor certification to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(instructor)))
}

/// Get all instructors with optional filtering
async fn get_instructors(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(query): Query<TrainingQuery>,
) -> Result<Json<ApiResponse<Vec<TrainingInstructor>>>, ApiError> {
    let instructors = state.db.get_training_instructors(&query).map_err(|e| {
        tracing::error!("Failed to get instructors: {}", e);
        ApiError::InternalServerError("Failed to retrieve instructors".to_string())
    })?;

    Ok(Json(ApiResponse::success(instructors)))
}

/// Revoke instructor certification (Staff only)
async fn revoke_instructor_certification(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path(instructor_id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    // Get instructor info for audit log before revocation
    state
        .db
        .revoke_instructor_certification(instructor_id)
        .map_err(|e| {
            tracing::error!("Failed to revoke instructor certification: {}", e);
            ApiError::InternalServerError("Failed to revoke instructor certification".to_string())
        })?;

    // Log the instructor revocation to audit logs
    if let Err(e) = state
        .audit_logger
        .log_event(
            crate::models::AuditEventType::InstructorRevoked,
            None,
            Some(_staff.0.id),
            serde_json::json!({
                "instructor_id": instructor_id,
            }),
            None,
            Some(format!("Instructor certification revoked")),
        )
        .await
    {
        tracing::warn!("Failed to log instructor revocation to audit: {}", e);
    }

    Ok(Json(ApiResponse::success(())))
}

// ==================== TOOL ACCESS VALIDATION ====================

/// Check if a specific user can access a tool (Staff only)
async fn check_tool_access(
    State(state): State<AppState>,
    _staff: StaffUser,
    Path((tool_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<bool>>, ApiError> {
    let can_access = state.db.can_access_tool(user_id, tool_id).map_err(|e| {
        // See the note in update_tool. A tool or user that does not exist is a
        // 404, not a server fault.
        tracing::warn!("can_access_tool({user_id}, {tool_id}) failed: {e}");
        ApiError::from(e)
    })?;

    Ok(Json(ApiResponse::success(can_access)))
}

/// Check if the current user can access a tool
async fn check_my_tool_access(
    State(state): State<AppState>,
    user: AuthUser,
    Path(tool_id): Path<Uuid>,
) -> Result<Json<ApiResponse<bool>>, ApiError> {
    let can_access = state.db.can_access_tool(user.0.id, tool_id).map_err(|e| {
        tracing::error!("Failed to check tool access: {}", e);
        ApiError::InternalServerError("Failed to check tool access".to_string())
    })?;

    Ok(Json(ApiResponse::success(can_access)))
}
