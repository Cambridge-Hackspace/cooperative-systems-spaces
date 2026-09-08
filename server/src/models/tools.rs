use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Tool status enum matching the database enum
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow,
)]
#[diesel(sql_type = crate::schema::sql_types::ToolStatus)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Idle,
    InUse,
    Maintenance,
    Broken,
    Repair,
    Retired,
}

/// Tool category enum matching the database enum
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow,
)]
#[diesel(sql_type = crate::schema::sql_types::ToolCategory)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Saw,
    #[serde(rename = "powertool")]
    PowerTool,
    #[serde(rename = "hand_tools")]
    HandTools,
    Measuring,
    Safety,
    Electronics,
    Woodworking,
    Metalworking,
    #[serde(rename = "3d_printing")]
    ThreeDPrinting,
    #[serde(rename = "laser_cutting")]
    LaserCutting,
    Welding,
    Other,
}

impl ToolStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolStatus::Idle => "idle",
            ToolStatus::InUse => "in_use",
            ToolStatus::Maintenance => "maintenance",
            ToolStatus::Broken => "broken",
            ToolStatus::Repair => "repair",
            ToolStatus::Retired => "retired",
        }
    }
}

impl ToolCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolCategory::Saw => "saw",
            ToolCategory::PowerTool => "powertool",
            ToolCategory::HandTools => "hand_tools",
            ToolCategory::Measuring => "measuring",
            ToolCategory::Safety => "safety",
            ToolCategory::Electronics => "electronics",
            ToolCategory::Woodworking => "woodworking",
            ToolCategory::Metalworking => "metalworking",
            ToolCategory::ThreeDPrinting => "3d_printing",
            ToolCategory::LaserCutting => "laser_cutting",
            ToolCategory::Welding => "welding",
            ToolCategory::Other => "other",
        }
    }
}

// Diesel serialization implementations for ToolStatus
impl diesel::serialize::ToSql<crate::schema::sql_types::ToolStatus, diesel::pg::Pg> for ToolStatus {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        use std::io::Write;
        match self {
            ToolStatus::Idle => out.write_all(b"idle")?,
            ToolStatus::InUse => out.write_all(b"in_use")?,
            ToolStatus::Maintenance => out.write_all(b"maintenance")?,
            ToolStatus::Broken => out.write_all(b"broken")?,
            ToolStatus::Repair => out.write_all(b"repair")?,
            ToolStatus::Retired => out.write_all(b"retired")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<crate::schema::sql_types::ToolStatus, diesel::pg::Pg>
    for ToolStatus
{
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"idle" => Ok(ToolStatus::Idle),
            b"in_use" => Ok(ToolStatus::InUse),
            b"maintenance" => Ok(ToolStatus::Maintenance),
            b"broken" => Ok(ToolStatus::Broken),
            b"repair" => Ok(ToolStatus::Repair),
            b"retired" => Ok(ToolStatus::Retired),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// Diesel serialization implementations for ToolCategory
impl diesel::serialize::ToSql<crate::schema::sql_types::ToolCategory, diesel::pg::Pg>
    for ToolCategory
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        use std::io::Write;
        match self {
            ToolCategory::Saw => out.write_all(b"saw")?,
            ToolCategory::PowerTool => out.write_all(b"powertool")?,
            ToolCategory::HandTools => out.write_all(b"hand_tools")?,
            ToolCategory::Measuring => out.write_all(b"measuring")?,
            ToolCategory::Safety => out.write_all(b"safety")?,
            ToolCategory::Electronics => out.write_all(b"electronics")?,
            ToolCategory::Woodworking => out.write_all(b"woodworking")?,
            ToolCategory::Metalworking => out.write_all(b"metalworking")?,
            ToolCategory::ThreeDPrinting => out.write_all(b"3d_printing")?,
            ToolCategory::LaserCutting => out.write_all(b"laser_cutting")?,
            ToolCategory::Welding => out.write_all(b"welding")?,
            ToolCategory::Other => out.write_all(b"other")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<crate::schema::sql_types::ToolCategory, diesel::pg::Pg>
    for ToolCategory
{
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"saw" => Ok(ToolCategory::Saw),
            b"powertool" => Ok(ToolCategory::PowerTool),
            b"hand_tools" => Ok(ToolCategory::HandTools),
            b"measuring" => Ok(ToolCategory::Measuring),
            b"safety" => Ok(ToolCategory::Safety),
            b"electronics" => Ok(ToolCategory::Electronics),
            b"woodworking" => Ok(ToolCategory::Woodworking),
            b"metalworking" => Ok(ToolCategory::Metalworking),
            b"3d_printing" => Ok(ToolCategory::ThreeDPrinting),
            b"laser_cutting" => Ok(ToolCategory::LaserCutting),
            b"welding" => Ok(ToolCategory::Welding),
            b"other" => Ok(ToolCategory::Other),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

/// Tool model for database operations
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tools)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Tool {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub category: ToolCategory,
    pub status: ToolStatus,
    pub barcode: Option<String>,
    pub serial_number: Option<String>,
    pub location: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub purchase_price: Option<BigDecimal>,
    pub maintenance_notes: Option<String>,
    pub requires_training: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub external_id: Option<String>,
    pub external_api_key: Option<String>,
    pub place_id: Option<Uuid>,
    /// Optional usability window. When set and the schedule isn't currently
    /// open, the tool is effectively unavailable.
    pub schedule_id: Option<Uuid>,
    /// Metered-billing rates (Phase 2). A tool is "metered" iff a flat fee or a
    /// per-minute rate is set; otherwise it is free (training-gated as before).
    /// Appended last for the positional-`Queryable` reason `email_verified_at`
    /// documents on the `User` model.
    pub usage_flat_fee: Option<BigDecimal>,
    pub usage_rate_per_min: Option<BigDecimal>,
    /// Caps billable time and the prepaid hold estimate; `None` falls back to the
    /// global `[tool_billing].default_max_session_minutes`.
    pub usage_max_session_minutes: Option<i32>,
}

/// New tool for creation
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tools)]
pub struct NewTool {
    pub name: String,
    pub description: Option<String>,
    pub category: ToolCategory,
    pub status: Option<ToolStatus>,
    pub barcode: Option<String>,
    pub serial_number: Option<String>,
    pub location: Option<String>,
    pub purchase_date: Option<NaiveDate>,
    pub purchase_price: Option<BigDecimal>,
    pub maintenance_notes: Option<String>,
    pub requires_training: Option<bool>,
    pub created_by: Uuid,
    pub external_id: Option<String>,
    pub external_api_key: Option<String>,
    pub place_id: Option<Uuid>,
    pub schedule_id: Option<Uuid>,
    pub usage_flat_fee: Option<BigDecimal>,
    pub usage_rate_per_min: Option<BigDecimal>,
    pub usage_max_session_minutes: Option<i32>,
}

/// Tool event model
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_events)]
pub struct ToolEvent {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub event_type: String,
    pub old_status: Option<ToolStatus>,
    pub new_status: Option<ToolStatus>,
    pub user_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub notes: Option<String>,
    pub scan_data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// New tool event
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_events)]
pub struct NewToolEvent {
    pub tool_id: Uuid,
    pub event_type: String,
    pub old_status: Option<ToolStatus>,
    pub new_status: Option<ToolStatus>,
    pub user_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub notes: Option<String>,
    pub scan_data: Option<serde_json::Value>,
}

/// Tool training type model
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::tool_training_types)]
pub struct ToolTrainingType {
    pub id: Uuid,
    pub tool_id: Uuid,
    pub training_name: String,
    pub description: Option<String>,
    pub expires_after_days: Option<i32>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// User tool training record
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::user_tool_training)]
pub struct UserToolTraining {
    pub id: Uuid,
    pub user_id: Uuid,
    pub training_type_id: Uuid,
    pub trainer_id: Uuid,
    pub trained_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

// /// Tool trainer authorization
// #[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
// #[diesel(table_name = crate::schema::tool_trainers)]
// pub struct ToolTrainer {
//     pub id: Uuid,
//     pub user_id: Uuid,
//     pub tool_id: Uuid,
//     pub authorized_by: Uuid,
//     pub authorized_at: DateTime<Utc>,
//     pub notes: Option<String>,
// }
