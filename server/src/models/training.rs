use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Training status enum matching the database enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow)]
#[diesel(sql_type = crate::schema::sql_types::TrainingStatus)]
#[serde(rename_all = "snake_case")]
pub enum TrainingStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed,
    Expired,
}

/// Assessment type enum matching the database enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow)]
#[diesel(sql_type = crate::schema::sql_types::AssessmentType)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentType {
    Practical,
    Written,
    Both,
    ObservationOnly,
}

impl TrainingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrainingStatus::NotStarted => "not_started",
            TrainingStatus::InProgress => "in_progress",
            TrainingStatus::Completed => "completed",
            TrainingStatus::Failed => "failed",
            TrainingStatus::Expired => "expired",
        }
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, TrainingStatus::Completed)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, TrainingStatus::InProgress | TrainingStatus::Completed)
    }
}

impl AssessmentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssessmentType::Practical => "practical",
            AssessmentType::Written => "written",
            AssessmentType::Both => "both",
            AssessmentType::ObservationOnly => "observation_only",
        }
    }

    pub fn requires_written_test(&self) -> bool {
        matches!(self, AssessmentType::Written | AssessmentType::Both)
    }

    pub fn requires_practical_test(&self) -> bool {
        matches!(self, AssessmentType::Practical | AssessmentType::Both)
    }
}

// Diesel serialization implementations for TrainingStatus
impl diesel::serialize::ToSql<crate::schema::sql_types::TrainingStatus, diesel::pg::Pg> for TrainingStatus {
    fn to_sql<'b>(&'b self, out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>) -> diesel::serialize::Result {
        use std::io::Write;
        match self {
            TrainingStatus::NotStarted => out.write_all(b"not_started")?,
            TrainingStatus::InProgress => out.write_all(b"in_progress")?,
            TrainingStatus::Completed => out.write_all(b"completed")?,
            TrainingStatus::Failed => out.write_all(b"failed")?,
            TrainingStatus::Expired => out.write_all(b"expired")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<crate::schema::sql_types::TrainingStatus, diesel::pg::Pg> for TrainingStatus {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"not_started" => Ok(TrainingStatus::NotStarted),
            b"in_progress" => Ok(TrainingStatus::InProgress),
            b"completed" => Ok(TrainingStatus::Completed),
            b"failed" => Ok(TrainingStatus::Failed),
            b"expired" => Ok(TrainingStatus::Expired),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// Diesel serialization implementations for AssessmentType
impl diesel::serialize::ToSql<crate::schema::sql_types::AssessmentType, diesel::pg::Pg> for AssessmentType {
    fn to_sql<'b>(&'b self, out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>) -> diesel::serialize::Result {
        use std::io::Write;
        match self {
            AssessmentType::Practical => out.write_all(b"practical")?,
            AssessmentType::Written => out.write_all(b"written")?,
            AssessmentType::Both => out.write_all(b"both")?,
            AssessmentType::ObservationOnly => out.write_all(b"observation_only")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<crate::schema::sql_types::AssessmentType, diesel::pg::Pg> for AssessmentType {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"practical" => Ok(AssessmentType::Practical),
            b"written" => Ok(AssessmentType::Written),
            b"both" => Ok(AssessmentType::Both),
            b"observation_only" => Ok(AssessmentType::ObservationOnly),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

/// Sequential training step for a tool
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_steps)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TrainingStep {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub step_number: i32,
    pub step_name: String,
    pub description: Option<String>,
    pub training_materials_url: Option<String>,
    pub requires_assessment: bool,
    pub assessment_type: Option<AssessmentType>,
    pub duration_minutes: Option<i32>,
    pub expires_after_days: Option<i32>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// New training step for creation
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_steps)]
pub struct NewTrainingStep {
    pub tool_id: Uuid,
    pub step_number: i32,
    pub step_name: String,
    pub description: Option<String>,
    pub training_materials_url: Option<String>,
    pub requires_assessment: Option<bool>,
    pub assessment_type: Option<AssessmentType>,
    pub duration_minutes: Option<i32>,
    pub expires_after_days: Option<i32>,
    pub created_by: Uuid,
}

/// Update training step request
#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_steps, treat_none_as_null = false)]
pub struct UpdateTrainingStep {
    pub step_name: Option<String>,
    pub description: Option<String>,
    pub training_materials_url: Option<String>,
    pub requires_assessment: Option<bool>,
    pub assessment_type: Option<AssessmentType>,
    pub duration_minutes: Option<i32>,
    pub expires_after_days: Option<i32>,
}

/// Training prerequisite relationship
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_prerequisites)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TrainingPrerequisite {
    pub id: Uuid,
    pub training_step_id: Uuid,
    pub prerequisite_step_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// New training prerequisite
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_prerequisites)]
pub struct NewTrainingPrerequisite {
    pub training_step_id: Uuid,
    pub prerequisite_step_id: Uuid,
}

/// User progress through a training step
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::user_training_progress)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserTrainingProgress {
    pub id: Uuid,
    pub user_id: Uuid,
    pub training_step_id: Uuid,
    pub status: TrainingStatus,
    pub instructor_id: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub assessment_score: Option<i32>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// New user training progress record
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::user_training_progress)]
pub struct NewUserTrainingProgress {
    pub user_id: Uuid,
    pub training_step_id: Uuid,
    pub status: Option<TrainingStatus>,
    pub instructor_id: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

/// Update user training progress
#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize, Default)]
#[diesel(table_name = crate::schema::user_training_progress, treat_none_as_null = false)]
pub struct UpdateUserTrainingProgress {
    pub status: Option<TrainingStatus>,
    pub instructor_id: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub assessment_score: Option<i32>,
    pub notes: Option<String>,
}

/// Training instructor certification
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_instructors)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TrainingInstructor {
    pub id: Uuid,
    pub user_id: Uuid,
    pub training_step_id: Uuid,
    pub certified_by: Uuid,
    pub certified_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// New training instructor certification
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_instructors)]
pub struct NewTrainingInstructor {
    pub user_id: Uuid,
    pub training_step_id: Uuid,
    pub certified_by: Uuid,
    pub expires_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

/// Combined training step with progress info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingStepWithProgress {
    pub step: TrainingStep,
    pub prerequisites: Vec<TrainingStep>,
    pub user_progress: Option<UserTrainingProgress>,
    pub is_available: bool, // true if all prerequisites are completed
    pub instructor_required: bool, // true if user needs instructor to proceed
}

/// Training overview for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTrainingOverview {
    pub tool_id: Uuid,
    pub tool_name: String,
    pub steps: Vec<TrainingStepWithProgress>,
    pub overall_progress: f32, // 0.0 to 1.0 percentage complete
    pub can_access_tool: bool, // true if all required training is complete
    pub next_step: Option<TrainingStep>, // next available training step
}

/// Training session request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartTrainingRequest {
    pub training_step_id: Uuid,
    pub instructor_id: Option<Uuid>, // Some for instructor-led, None for self-study
    pub notes: Option<String>,
}

/// Complete training request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteTrainingRequest {
    pub training_step_id: Uuid,
    pub assessment_score: Option<i32>, // 0-100 if assessment was graded
    pub passed: bool, // true if training was successfully completed
    pub notes: Option<String>,
}

impl TrainingStep {
    /// Calculate when this training would expire if completed now
    pub fn calculate_expiry_date(&self) -> Option<DateTime<Utc>> {
        self.expires_after_days.map(|days| {
            Utc::now() + chrono::Duration::days(days as i64)
        })
    }

    /// Check if this training step requires an instructor
    pub fn requires_instructor(&self) -> bool {
        self.requires_assessment && 
        matches!(self.assessment_type, Some(AssessmentType::Practical) | Some(AssessmentType::Both))
    }

    /// Get a human-readable duration
    pub fn formatted_duration(&self) -> String {
        match self.duration_minutes {
            Some(minutes) if minutes >= 60 => {
                let hours = minutes / 60;
                let remaining_minutes = minutes % 60;
                if remaining_minutes > 0 {
                    format!("{}h {}m", hours, remaining_minutes)
                } else {
                    format!("{}h", hours)
                }
            }
            Some(minutes) => format!("{}m", minutes),
            None => "Unknown duration".to_string(),
        }
    }
}

impl UserTrainingProgress {
    /// Check if this training certification is currently valid
    pub fn is_valid(&self) -> bool {
        self.status == TrainingStatus::Completed && 
        self.expires_at.map_or(true, |expiry| expiry > Utc::now())
    }

    /// Check if this training has expired
    pub fn is_expired(&self) -> bool {
        self.status == TrainingStatus::Completed &&
        self.expires_at.map_or(false, |expiry| expiry <= Utc::now())
    }

    /// Get the completion percentage (0.0 to 1.0)
    pub fn completion_percentage(&self) -> f32 {
        match self.status {
            TrainingStatus::NotStarted => 0.0,
            TrainingStatus::InProgress => 0.5,
            TrainingStatus::Completed => 1.0,
            TrainingStatus::Failed => 0.0,
            TrainingStatus::Expired => 0.0,
        }
    }
}