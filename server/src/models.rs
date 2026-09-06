mod account_tokens;
mod cmi5;
mod devices;
mod doors;
mod home_links;
mod mfa;
mod places;
mod profile_config;
mod schedules;
mod tools;
// `pub`, not `pub(crate)`: these types appear in the public signatures of
// handlers and models reachable through AppState, and a public item exposing a
// crate-private type trips `private_interfaces`, which is a hard error under
// -D warnings. It also has to be reachable from server/tests/.
pub mod trainers;
mod training;
mod webhooks;

pub use account_tokens::*;
pub use cmi5::*;
pub use devices::*;
pub use doors::*;
pub use home_links::*;
pub use mfa::*;
pub use places::*;
pub use profile_config::*;
pub use schedules::*;
pub use tools::*;
pub use trainers::*;
pub use training::*;
pub use webhooks::*;

use crate::schema::{audit_logs, groupsio_sync_runs, sql_types, users};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

/// User role enum for granular permissions
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, diesel::AsExpression, diesel::FromSqlRow,
)]
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
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
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
    /// When the address was confirmed. `None` means unconfirmed.
    ///
    /// Last in the struct because `ALTER TABLE ADD COLUMN` appends physically
    /// and `Queryable` loads positionally -- a field inserted in the middle
    /// here would silently start reading the wrong column.
    ///
    /// Accounts predating the column were backfilled as confirmed by the
    /// migration, so turning `auth.require_email_verification` on does not lock
    /// out an existing membership.
    pub email_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When the member opted out of the Groups.io mailing list. `None` means
    /// subscribed-by-default: they are on the list once the account is active
    /// and the email is verified. Set from the platform toggle or on learning
    /// of an unsubscribe done from a Groups.io email link.
    ///
    /// Last in the struct for the same reason as `email_verified_at`:
    /// `ALTER TABLE ADD COLUMN` appends physically and `Queryable` loads
    /// positionally, so a new column must be the last field here too.
    pub mailing_list_opt_out_at: Option<chrono::DateTime<chrono::Utc>>,
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

    pub fn with_role(
        username: String,
        email: String,
        password_hash: String,
        full_name: String,
        role: UserRole,
    ) -> Self {
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

/// One recorded Groups.io reconciliation pass, for the admin status view.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = groupsio_sync_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct GroupsioSyncRun {
    pub id: uuid::Uuid,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
    pub added: i32,
    pub removed: i32,
    pub opted_out: i32,
    pub ok: bool,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A reconciliation pass to record.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = groupsio_sync_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewGroupsioSyncRun {
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: chrono::DateTime<chrono::Utc>,
    pub added: i32,
    pub removed: i32,
    pub opted_out: i32,
    pub ok: bool,
    pub error: Option<String>,
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
    /// A trainee confirming, in their own name, that they have read a
    /// step's safety documentation. Distinct from
    /// TrainingSessionCompleted because the actor is the subject: it is
    /// the difference between a record of what somebody attested to and a
    /// record of what somebody was signed off for, and only the first is
    /// worth anything if it is ever produced as evidence.
    TrainingDocumentationAcknowledged,
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
    // Transactional email and account recovery
    //
    // Appended at the tail of all three lists rather than filed next to the
    // other user events, so that a concurrent branch inserting near the
    // training group does not collide here. Grouping by subject would be
    // tidier; not conflicting with work already in review is worth more.
    /// A reset was asked for. The payload records whether an account was
    /// found -- this is the one place that answer is written down, because the
    /// endpoint deliberately will not tell the requester.
    PasswordResetRequested,
    /// A password was changed without the old one being presented.
    PasswordResetCompleted,
    /// A reset token was rejected: unknown, expired, or already spent. Volume
    /// here is the only brute-force signal a public endpoint gives off.
    PasswordResetFailed,
    EmailVerificationSent,
    EmailVerified,
    /// The mailer could not deliver. The request that triggered it cannot
    /// report this -- saying so would turn a send failure into an account
    /// enumeration oracle -- so this row is how an operator finds out.
    EmailSendFailed,
    // cmi5 training modules
    //
    // Appended at the tail for the same reason as the block above: a new event
    // type filed next to a related group is tidier but collides with any branch
    // touching that group.
    Cmi5CoursePublished,
    Cmi5CourseDeleted,
    Cmi5AuAssignedToTool,
    Cmi5Launched,
    /// A verified cmi5 pass satisfied an AU and granted the mapped training
    /// step. This is the cmi5 analogue of `training_session_completed`: the
    /// record that a browser course led to physical tool access.
    Cmi5AuSatisfied,
    Cmi5CourseExported,
    // Groups.io mailing-list opt-in/opt-out
    //
    // Appended at the tail, like the transactional-email group above, so a
    // concurrent branch adding events near another group does not collide here.
    /// A member asked, through the platform, to be on the mailing list (or had
    /// their opt-out cleared). The Groups.io sync consumes this to add them.
    MailingListSubscribe,
    /// A member opted out of the mailing list -- via the platform toggle, or on
    /// the platform learning of an unsubscribe done from a Groups.io email link.
    /// The sync consumes this to remove them and never re-add.
    MailingListUnsubscribe,
    /// Reconciliation added an address to the Groups.io group to match intended
    /// membership. A record of what the sync did, not a member action.
    MailingListSyncAdd,
    /// Reconciliation removed an address from the Groups.io group -- either it
    /// was no longer intended, or (platform owns the list) it was a subscriber
    /// the platform did not add. A record of what the sync did.
    MailingListSyncRemove,
    /// A user's email address was changed. Emitted from the account-update path,
    /// which previously recorded nothing -- so a change there would silently
    /// desync the mailing list. The payload carries the old and new addresses.
    UserEmailChange,
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
            Self::TrainingDocumentationAcknowledged => "training_documentation_acknowledged",
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
            Self::PasswordResetRequested => "password_reset_requested",
            Self::PasswordResetCompleted => "password_reset_completed",
            Self::PasswordResetFailed => "password_reset_failed",
            Self::EmailVerificationSent => "email_verification_sent",
            Self::EmailVerified => "email_verified",
            Self::EmailSendFailed => "email_send_failed",
            Self::Cmi5CoursePublished => "cmi5_course_published",
            Self::Cmi5CourseDeleted => "cmi5_course_deleted",
            Self::Cmi5AuAssignedToTool => "cmi5_au_assigned_to_tool",
            Self::Cmi5Launched => "cmi5_launched",
            Self::Cmi5AuSatisfied => "cmi5_au_satisfied",
            Self::Cmi5CourseExported => "cmi5_course_exported",
            Self::MailingListSubscribe => "mailing_list_subscribe",
            Self::MailingListUnsubscribe => "mailing_list_unsubscribe",
            Self::MailingListSyncAdd => "mailing_list_sync_add",
            Self::MailingListSyncRemove => "mailing_list_sync_remove",
            Self::UserEmailChange => "user_email_change",
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
            TrainingDocumentationAcknowledged,
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
            PasswordResetRequested,
            PasswordResetCompleted,
            PasswordResetFailed,
            EmailVerificationSent,
            EmailVerified,
            EmailSendFailed,
            Cmi5CoursePublished,
            Cmi5CourseDeleted,
            Cmi5AuAssignedToTool,
            Cmi5Launched,
            Cmi5AuSatisfied,
            Cmi5CourseExported,
            MailingListSubscribe,
            MailingListUnsubscribe,
            MailingListSyncAdd,
            MailingListSyncRemove,
            UserEmailChange,
        ]
    }
}
