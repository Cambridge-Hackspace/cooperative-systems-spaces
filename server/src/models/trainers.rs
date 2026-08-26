use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ==================== TOOL TRAINERS ====================

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_trainers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ToolTrainer {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tool_id: Uuid,
    pub authorized_by: Uuid,
    pub authorized_at: DateTime<Utc>,
    pub notes: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_trainers)]
pub struct NewToolTrainer {
    pub user_id: Uuid,
    pub tool_id: Uuid,
    pub authorized_by: Uuid,
    pub notes: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_trainers, treat_none_as_null = false)]
pub struct UpdateToolTrainer {
    pub notes: Option<String>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub is_active: Option<bool>,
}

// ==================== TRAINING RECORDS ====================

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_records)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TrainingRecord {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub trainee_user_id: Uuid,
    pub trainer_user_id: Uuid,
    pub training_date: NaiveDate,
    pub completion_status: String,
    pub minutes_trained: Option<i32>,
    pub skills_covered: Option<Vec<Option<String>>>,
    pub notes: Option<String>,
    pub next_steps: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub training_step_id: Option<Uuid>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_records)]
pub struct NewTrainingRecord {
    pub tool_id: Uuid,
    pub training_step_id: Option<Uuid>,
    pub trainee_user_id: Uuid,
    pub trainer_user_id: Uuid,
    pub training_date: NaiveDate,
    pub completion_status: String,
    pub minutes_trained: Option<i32>,
    pub skills_covered: Option<Vec<Option<String>>>,
    pub notes: Option<String>,
    pub next_steps: Option<String>,
}

#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::training_records, treat_none_as_null = false)]
pub struct UpdateTrainingRecord {
    pub completion_status: Option<String>,
    pub minutes_trained: Option<i32>,
    pub training_step_id: Option<Uuid>,
    pub skills_covered: Option<Vec<Option<String>>>,
    pub notes: Option<String>,
    pub next_steps: Option<String>,
}

// ==================== COMBINED/RESPONSE TYPES ====================

/// Tool trainer with user information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolTrainerWithUser {
    pub trainer: ToolTrainer,
    pub user_name: String,
    pub user_email: String,
    pub user_full_name: Option<String>,
}

/// Training record with user information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRecordWithUsers {
    pub record: TrainingRecord,
    pub trainee_name: String,
    pub trainer_name: String,
    pub tool_name: String,
}

/// Training completion status enum for validation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrainingCompletionStatus {
    Completed,
    Partial,
    Failed,
}

impl TrainingCompletionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for TrainingCompletionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<String> for TrainingCompletionStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "completed" => Self::Completed,
            "partial" => Self::Partial,
            "failed" => Self::Failed,
            _ => Self::Failed, // Default to failed for invalid values
        }
    }
}

// ==================== IMPLEMENTATION BLOCKS ====================

impl ToolTrainer {
    /// Check if the trainer authorization has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false // Never expires if expires_at is None
        }
    }

    /// Check if the trainer is currently active and not expired
    pub fn is_currently_active(&self) -> bool {
        self.is_active && !self.is_expired()
    }
}

impl TrainingRecord {
    /// Get a human-readable training duration
    pub fn formatted_duration(&self) -> String {
        match self.minutes_trained {
            Some(minutes) => {
                if minutes == 0 {
                    "< 1 minute".to_string()
                } else if minutes < 60 {
                    format!("{} minutes", minutes)
                } else {
                    let hours = minutes / 60;
                    let remaining_minutes = minutes % 60;
                    if remaining_minutes == 0 {
                        format!("{} hours", hours)
                    } else {
                        format!("{} hours {} minutes", hours, remaining_minutes)
                    }
                }
            }
            None => "Duration not recorded".to_string(),
        }
    }

    /// Check if this was a successful training session
    pub fn was_successful(&self) -> bool {
        self.completion_status == "completed"
    }
}
