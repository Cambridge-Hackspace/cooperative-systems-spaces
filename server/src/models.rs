mod tools;
mod training;
pub(crate) mod trainers;
mod devices;
mod webhooks;
mod mfa;
mod doors;
mod places;
mod schedules;
mod home_links;
mod profile_config;

pub use tools::*;
pub use training::*;
pub use trainers::*;
pub use devices::*;
pub use webhooks::*;
pub use mfa::*;
pub use doors::*;
pub use places::*;
pub use schedules::*;
pub use home_links::*;
pub use profile_config::*;

use std::io::Write;
use crate::schema::{users, audit_logs, sql_types};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User role enum for granular permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow)]
#[diesel(sql_type = sql_types::UserRole)]
pub enum UserRole {
    /// User status unknown (default for security)
    Unknown,
    /// New user, basic access only
    Newbie,
    /// Full member with standard access
    Member,
    /// Staff member with elevated privileges
    Staff,
    /// Administrator with full access
    Admin,
}

// Implement Diesel traits for UserRole enum
impl diesel::serialize::ToSql<sql_types::UserRole, diesel::pg::Pg> for UserRole {
    fn to_sql<'b>(&'b self, out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>) -> diesel::serialize::Result {
        match self {
            UserRole::Unknown => out.write_all(b"unknown")?,
            UserRole::Newbie => out.write_all(b"newbie")?,
            UserRole::Member => out.write_all(b"member")?,
            UserRole::Staff => out.write_all(b"staff")?,
            UserRole::Admin => out.write_all(b"admin")?,
        }
        Ok(diesel::serialize::IsNull::No)
    }
}

impl diesel::deserialize::FromSql<sql_types::UserRole, diesel::pg::Pg> for UserRole {
    fn from_sql(bytes: diesel::pg::PgValue<'_>) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"unknown" => Ok(UserRole::Unknown),
            b"newbie" => Ok(UserRole::Newbie),
            b"member" => Ok(UserRole::Member),
            b"staff" => Ok(UserRole::Staff),
            b"admin" => Ok(UserRole::Admin),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

impl Default for UserRole {
    fn default() -> Self {
        UserRole::Unknown
    }
}

impl UserRole {
    /// Check if user has admin privileges
    pub fn can_access_admin(&self) -> bool {
        matches!(self, UserRole::Admin)
    }

    /// Check if user has staff privileges
    pub fn can_access_staff(&self) -> bool {
        matches!(self, UserRole::Staff | UserRole::Admin)
    }

    /// Check if user has member privileges
    pub fn can_access_member(&self) -> bool {
        matches!(self, UserRole::Member | UserRole::Staff | UserRole::Admin)
    }

    pub fn is_active_user(&self) -> bool {
        !matches!(self, UserRole::Unknown)
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub full_name: String,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub role: UserRole,
    pub profile: serde_json::Value,
    pub meta: serde_json::Value,
    /// Set when the user confirms their first MFA method; cleared when the
    /// last method is removed. Used to short-circuit `has any MFA?` checks.
    pub mfa_enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewUser {
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub full_name: String,
    pub is_active: Option<bool>,
    pub role: Option<UserRole>,
    pub profile: Option<serde_json::Value>,
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, AsChangeset, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UpdateUser {
    pub username: Option<String>,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub full_name: Option<String>,
    pub is_active: Option<bool>,
    pub role: Option<UserRole>,
    pub profile: Option<serde_json::Value>,
    pub updated_at: Option<NaiveDateTime>,
    pub meta: Option<serde_json::Value>,
}

impl NewUser {
    pub fn new(username: String, email: String, password_hash: String, full_name: String) -> Self {
        Self {
            username,
            email,
            password_hash,
            full_name,
            is_active: Some(true),
            role: Some(UserRole::Newbie),
            profile: Some(serde_json::json!({})),
            meta: Some(serde_json::json!({})),
        }
    }

    pub fn with_role(username: String, email: String, password_hash: String, full_name: String, role: UserRole) -> Self {
        Self {
            username,
            email,
            password_hash,
            full_name,
            is_active: Some(true),
            role: Some(role),
            profile: Some(serde_json::json!({})),
            meta: Some(serde_json::json!({})),
        }
    }
}

/// Audit log entry for tracking user operations
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = audit_logs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct AuditLog {
    pub id: uuid::Uuid,
    pub event_type: String,
    pub user_id: Option<uuid::Uuid>,
    pub actor_id: Option<uuid::Uuid>,
    pub event_data: serde_json::Value,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// New audit log entry for insertion
#[derive(Debug, Clone, Insertable, Serialize, Deserialize)]
#[diesel(table_name = audit_logs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewAuditLog {
    pub event_type: String,
    pub user_id: Option<uuid::Uuid>,
    pub actor_id: Option<uuid::Uuid>,
    pub event_data: serde_json::Value,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

/// Audit event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    UserRegistration,
    UserLogin,
    UserLogout,
    UserRoleChange,
    UserProfileUpdate,
    UserPasswordChange,
    UserActivation,
    UserDeactivation,
    UserDeletion,
    AdminConfigReload,
    ProfileConfigUpdated,
    ProfileConfigRolledBack,
    FailedLoginAttempt,
    // Training-related events
    TrainingSessionStarted,
    TrainingSessionCompleted,
    TrainingStepCreated,
    TrainingStepUpdated,
    TrainingStepDeleted,
    TrainerAssigned,
    TrainerRemoved,
    InstructorCertified,
    InstructorRevoked,
    // ToolPass/Tool usage events
    ToolAccessGranted,
    ToolAccessDenied,
    ToolActivated,
    ToolDeactivated,
    ToolUsageLogged,
    // Device-related events
    DeviceInviteCreated,
    DeviceInviteUsed,
    DeviceInviteExpired,
    DeviceRegistered,
    DeviceNameChanged,
    DeviceDeleted,
    DeviceVersionChanged,
    // Webhook management events
    WebhookCreated,
    WebhookUpdated,
    WebhookDeleted,
    WebhookAuthHeaderCreated,
    WebhookAuthHeaderUpdated,
    WebhookAuthHeaderDeleted,
    // MFA events
    MfaTotpEnrolled,
    MfaTotpDisabled,
    MfaWebauthnRegistered,
    MfaWebauthnRemoved,
    MfaRecoveryCodesRegenerated,
    MfaRecoveryCodeUsed,
    MfaLoginPassed,
    MfaLoginFailed,
    // Door events
    DoorCreated,
    DoorUpdated,
    DoorDeleted,
    DoorRuleAdded,
    DoorRuleRemoved,
    DoorUnlockedCard,
    DoorUnlockedQr,
    DoorUnlockedAdmin,
    DoorUnlockDenied,
    DoorCheckinRecorded,
    // Place events
    PlaceCreated,
    PlaceUpdated,
    PlaceMoved,
    PlaceDeleted,
    // Schedule events
    ScheduleCreated,
    ScheduleUpdated,
    ScheduleDeleted,
    // Home links
    HomeLinkCreated,
    HomeLinkUpdated,
    HomeLinkDeleted,
}

impl AuditEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserRegistration => "user_registration",
            Self::UserLogin => "user_login",
            Self::UserLogout => "user_logout",
            Self::UserRoleChange => "user_role_change",
            Self::UserProfileUpdate => "user_profile_update",
            Self::UserPasswordChange => "user_password_change",
            Self::UserActivation => "user_activation",
            Self::UserDeactivation => "user_deactivation",
            Self::UserDeletion => "user_deletion",
            Self::AdminConfigReload => "admin_config_reload",
            Self::ProfileConfigUpdated => "profile_config_updated",
            Self::ProfileConfigRolledBack => "profile_config_rolled_back",
            Self::FailedLoginAttempt => "failed_login_attempt",
            Self::TrainingSessionStarted => "training_session_started",
            Self::TrainingSessionCompleted => "training_session_completed",
            Self::TrainingStepCreated => "training_step_created",
            Self::TrainingStepUpdated => "training_step_updated",
            Self::TrainingStepDeleted => "training_step_deleted",
            Self::TrainerAssigned => "trainer_assigned",
            Self::TrainerRemoved => "trainer_removed",
            Self::InstructorCertified => "instructor_certified",
            Self::InstructorRevoked => "instructor_revoked",
            Self::ToolAccessGranted => "tool_access_granted",
            Self::ToolAccessDenied => "tool_access_denied",
            Self::ToolActivated => "tool_activated",
            Self::ToolDeactivated => "tool_deactivated",
            Self::ToolUsageLogged => "tool_usage_logged",
            Self::DeviceInviteCreated => "device_invite_created",
            Self::DeviceInviteUsed => "device_invite_used",
            Self::DeviceInviteExpired => "device_invite_expired",
            Self::DeviceRegistered => "device_registered",
            Self::DeviceNameChanged => "device_name_changed",
            Self::DeviceDeleted => "device_deleted",
            Self::DeviceVersionChanged => "device_version_changed",
            Self::WebhookCreated => "webhook_created",
            Self::WebhookUpdated => "webhook_updated",
            Self::WebhookDeleted => "webhook_deleted",
            Self::WebhookAuthHeaderCreated => "webhook_auth_header_created",
            Self::WebhookAuthHeaderUpdated => "webhook_auth_header_updated",
            Self::WebhookAuthHeaderDeleted => "webhook_auth_header_deleted",
            Self::MfaTotpEnrolled => "mfa_totp_enrolled",
            Self::MfaTotpDisabled => "mfa_totp_disabled",
            Self::MfaWebauthnRegistered => "mfa_webauthn_registered",
            Self::MfaWebauthnRemoved => "mfa_webauthn_removed",
            Self::MfaRecoveryCodesRegenerated => "mfa_recovery_codes_regenerated",
            Self::MfaRecoveryCodeUsed => "mfa_recovery_code_used",
            Self::MfaLoginPassed => "mfa_login_passed",
            Self::MfaLoginFailed => "mfa_login_failed",
            Self::DoorCreated => "door_created",
            Self::DoorUpdated => "door_updated",
            Self::DoorDeleted => "door_deleted",
            Self::DoorRuleAdded => "door_rule_added",
            Self::DoorRuleRemoved => "door_rule_removed",
            Self::DoorUnlockedCard => "door_unlocked_card",
            Self::DoorUnlockedQr => "door_unlocked_qr",
            Self::DoorUnlockedAdmin => "door_unlocked_admin",
            Self::DoorUnlockDenied => "door_unlock_denied",
            Self::DoorCheckinRecorded => "door_checkin_recorded",
            Self::PlaceCreated => "place_created",
            Self::PlaceUpdated => "place_updated",
            Self::PlaceMoved => "place_moved",
            Self::PlaceDeleted => "place_deleted",
            Self::ScheduleCreated => "schedule_created",
            Self::ScheduleUpdated => "schedule_updated",
            Self::ScheduleDeleted => "schedule_deleted",
            Self::HomeLinkCreated => "home_link_created",
            Self::HomeLinkUpdated => "home_link_updated",
            Self::HomeLinkDeleted => "home_link_deleted",
        }
    }

    /// All known audit event types, in stable display order.
    /// Used to populate the webhook event-subscription picker.
    pub fn all() -> &'static [AuditEventType] {
        use AuditEventType::*;
        &[
            UserRegistration,
            UserLogin,
            UserLogout,
            UserRoleChange,
            UserProfileUpdate,
            UserPasswordChange,
            UserActivation,
            UserDeactivation,
            UserDeletion,
            AdminConfigReload,
            ProfileConfigUpdated,
            ProfileConfigRolledBack,
            FailedLoginAttempt,
            TrainingSessionStarted,
            TrainingSessionCompleted,
            TrainingStepCreated,
            TrainingStepUpdated,
            TrainingStepDeleted,
            TrainerAssigned,
            TrainerRemoved,
            InstructorCertified,
            InstructorRevoked,
            ToolAccessGranted,
            ToolAccessDenied,
            ToolActivated,
            ToolDeactivated,
            ToolUsageLogged,
            DeviceInviteCreated,
            DeviceInviteUsed,
            DeviceInviteExpired,
            DeviceRegistered,
            DeviceNameChanged,
            DeviceDeleted,
            DeviceVersionChanged,
            WebhookCreated,
            WebhookUpdated,
            WebhookDeleted,
            WebhookAuthHeaderCreated,
            WebhookAuthHeaderUpdated,
            WebhookAuthHeaderDeleted,
            MfaTotpEnrolled,
            MfaTotpDisabled,
            MfaWebauthnRegistered,
            MfaWebauthnRemoved,
            MfaRecoveryCodesRegenerated,
            MfaRecoveryCodeUsed,
            MfaLoginPassed,
            MfaLoginFailed,
            DoorCreated,
            DoorUpdated,
            DoorDeleted,
            DoorRuleAdded,
            DoorRuleRemoved,
            DoorUnlockedCard,
            DoorUnlockedQr,
            DoorUnlockedAdmin,
            DoorUnlockDenied,
            DoorCheckinRecorded,
            PlaceCreated,
            PlaceUpdated,
            PlaceMoved,
            PlaceDeleted,
            ScheduleCreated,
            ScheduleUpdated,
            ScheduleDeleted,
            HomeLinkCreated,
            HomeLinkUpdated,
            HomeLinkDeleted,
        ]
    }
}
