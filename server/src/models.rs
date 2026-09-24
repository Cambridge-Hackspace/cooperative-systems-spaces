mod account_tokens;
mod cards;
mod cmi5;
mod devices;
mod doors;
mod home_links;
mod membership;
mod mfa;
mod places;
mod power;
mod profile_config;
mod schedules;
mod tool_billing;
mod tool_modules;
mod tool_tiers;
mod tools;
// `pub`, not `pub(crate)`: these types appear in the public signatures of
// handlers and models reachable through AppState, and a public item exposing a
// crate-private type trips `private_interfaces`, which is a hard error under
// -D warnings. It also has to be reachable from server/tests/.
mod training;
mod waivers;
mod webhooks;

pub use account_tokens::*;
pub use cards::*;
pub use cmi5::*;
pub use devices::*;
pub use doors::*;
pub use home_links::*;
pub use membership::*;
pub use mfa::*;
pub use places::*;
pub use power::*;
pub use profile_config::*;
pub use schedules::*;
pub use tool_billing::*;
pub use tool_modules::*;
pub use tool_tiers::*;
pub use tools::*;
pub use training::*;
pub use waivers::*;
pub use webhooks::*;

use crate::schema::{audit_logs, groupsio_sync_runs, users};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The canonical seeded role names, in ascending tier order. Authorization is
/// data-driven (see [`crate::rbac::RoleGraph`] + the `roles` table); these
/// constants exist only so app code that assigns a tier role -- registration,
/// the roster editor, the membership state machine -- refers to the seed by a
/// checked name rather than a bare string literal.
pub mod role {
    pub const GUEST: &str = "guest";
    pub const HISTORICAL: &str = "historical";
    pub const ACTIVE: &str = "active";
    pub const STAFF: &str = "staff";
    pub const ADMIN: &str = "admin";

    /// The tier roles a user can hold as their single primary role, lowest to
    /// highest. (Additional non-tier roles may also be granted via user_roles.)
    pub const TIERS: [&str; 5] = [GUEST, HISTORICAL, ACTIVE, STAFF, ADMIN];

    /// Whether `name` is one of the assignable tier roles.
    pub fn is_tier(name: &str) -> bool {
        TIERS.contains(&name)
    }

    /// The tier's rank on the `TIERS` ladder, 1 (guest) to 5 (admin); `None`
    /// for a name that is not a tier role. This mirrors the seeded RBAC levels
    /// but needs no database -- config validation at boot has no `RoleGraph`.
    pub fn tier_rank(name: &str) -> Option<usize> {
        TIERS.iter().position(|&t| t == name).map(|i| i + 1)
    }
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    /// Argon2 password hash. Skipped in serialization (#120/H9): `User` derives
    /// `Serialize` and is returned verbatim by the training-roster endpoints
    /// (`api/training.rs`), which bypass the hash-stripping `UserResponse` DTO
    /// that `api/users.rs` uses. Skipping the field here closes the leak for
    /// every serialization path, not just the two known endpoints; it is read
    /// by field access in the auth path, never out of JSON, so nothing breaks.
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub full_name: String,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
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
    /// Membership dues enrollment clock. `None` means the user is not enrolled
    /// in dues (a never-paid Newbie, or an honorary staff/admin), and the
    /// membership logic never touches them. `Some(t)` is the anniversary at
    /// which the next period's dues fall due; the renewal check evaluates it
    /// after `membership.grace_days`.
    ///
    /// Appended for the positional-`Queryable` reason above: new columns must be
    /// the last fields, in the same order as the migration's `ADD COLUMN`s.
    pub membership_next_due_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Stripe customer id, linking this user to the Billing Portal and mapping
    /// inbound webhooks back to exactly one account. Not card data.
    pub stripe_customer_id: Option<String>,
    /// The user's current Stripe subscription id, when they have a recurring
    /// membership. `None` for cash-only or one-shot members.
    pub stripe_subscription_id: Option<String>,
    /// Last-seen Stripe subscription status, kept for display only. The ledger
    /// balance -- not this -- is the entitlement gate.
    pub subscription_status: Option<String>,
    /// Session-revocation epoch (#120/M9). Every issued JWT carries this value;
    /// the auth extractor rejects a token whose version does not match, and a
    /// password change (self-service, admin reset, or reset token) bumps it,
    /// invalidating every session minted before the change. Last field for the
    /// positional-Queryable reason the columns above document.
    pub token_version: i32,
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
    /// An admin waived a member's training requirement for a tool (e.g. migrated
    /// access, external certification, staff discretion). Carries a reason.
    TrainingWaiverGranted,
    /// A previously-granted training waiver was revoked.
    TrainingWaiverRevoked,
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
    // Access card events
    /// A known-but-revoked (disabled or released) card was presented at a tool
    /// or door. Access is denied as usual; this distinct event exists so the
    /// attempt can be hooked for fraud alerting. An *unknown* code stays the
    /// ordinary quiet denial and does not emit this.
    RevokedCardPresented,
    /// An admin issued a new access card to a member.
    CardIssued,
    /// An admin disabled a member's card (lost/compromised/on-hold).
    CardDisabled,
    /// An admin released a member's card back to the pool for reissue.
    CardReleased,
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
    // Tool module bindings + safety interlocks (#83)
    ToolModuleCreated,
    ToolModuleDeleted,
    ToolInterlockCreated,
    ToolInterlockDeleted,
    // Bypass detection (#84). Transition events, not level events: a module
    // that has been silent for a week is one row, not one per sweep.
    ToolModuleSilent,
    ToolModuleReturned,
    UnauthorizedPowerDetected,
    MqttBrokerLost,
    MqttBrokerRestored,
    EdgeIsolationReported,
    // Power topology events (#42)
    PowerCircuitCreated,
    PowerCircuitUpdated,
    PowerCircuitDeleted,
    PowerOutletCreated,
    PowerOutletUpdated,
    PowerOutletDeleted,
    PowerReceptacleCreated,
    PowerReceptacleUpdated,
    PowerReceptacleDeleted,
    // Power interrupt / emergency lockout events (#44)
    CircuitOverageShutoff,
    EmergencyLockoutEngaged,
    EmergencyLockoutCleared,
    FirmwareSelftripReported,
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
    // Membership billing (Stripe + dues ledger)
    //
    // Appended at the tail, like the groups above, so a concurrent branch adding
    // events near another group does not collide here. These record membership
    // state changes and admin actions; their payloads carry amounts, status, and
    // Stripe reference ids but never card data.
    /// Membership was granted (role raised to the configured member role) because
    /// the balance covered the dues -- from a Stripe payment, a one-shot, or an
    /// admin-logged cash credit.
    MembershipGranted,
    /// Membership lapsed (role lowered to the configured lapsed role) because the
    /// balance could not cover the period's dues. No debt is carried.
    MembershipRevoked,
    /// An admin logged a manual (e.g. cash) payment as a ledger credit. The
    /// payload records the actor, subject, amount, and note -- accountability for
    /// money taken outside Stripe.
    MembershipPaymentRecorded,
    /// A lapse would have demoted the last remaining admin; the role change was
    /// refused. Recorded so the owner sees an admin who owes dues but was
    /// protected, rather than an unexplained non-event.
    MembershipLastAdminProtected,
    /// A Stripe subscription was created for a member (checkout completed in
    /// subscription mode).
    SubscriptionStarted,
    /// A Stripe subscription was canceled (via the Billing Portal or Stripe).
    SubscriptionCanceled,
    /// A Stripe invoice payment failed. Recorded for visibility; the lapse itself
    /// happens through the balance check, not this event.
    SubscriptionPaymentFailed,
    // Metered tool billing (Phase 2). Appended at the tail, like the groups
    // above. Payloads carry amounts/minutes/session ids, never card data.
    /// A metered tool-use session settled and posted a `tool_usage` ledger
    /// debit. The record of money charged for a tool use.
    ToolUsageCharged,
    /// The sweep closed a session that was never stopped (no `tool-off`),
    /// settling it from reported usage (or the cap).
    ToolSessionAbandoned,
    // Per-member rate tiers (#34). Rate changes are money changes, so who
    // created/edited a tier and who assigned a member to one is audited.
    ToolRateTierCreated,
    ToolRateTierUpdated,
    ToolRateTierDeleted,
    ToolTierAssigned,
    ToolTierUnassigned,
    // Data-driven RBAC administration (#65). Who changed a role, the permission
    // matrix, the inheritance graph, or a user's role assignments -- all of
    // which change who can do what, so all audited.
    RoleCreated,
    RoleUpdated,
    RoleDeleted,
    RolePermissionsChanged,
    RoleInheritanceChanged,
    UserRoleAssigned,
    UserRoleUnassigned,
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
            Self::TrainingWaiverGranted => "training_waiver_granted",
            Self::TrainingWaiverRevoked => "training_waiver_revoked",
            Self::TrainingDocumentationAcknowledged => "training_documentation_acknowledged",
            Self::ToolAccessGranted => "tool_access_granted",
            Self::ToolAccessDenied => "tool_access_denied",
            Self::ToolActivated => "tool_activated",
            Self::ToolDeactivated => "tool_deactivated",
            Self::ToolUsageLogged => "tool_usage_logged",
            Self::RevokedCardPresented => "revoked_card_presented",
            Self::CardIssued => "card_issued",
            Self::CardDisabled => "card_disabled",
            Self::CardReleased => "card_released",
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
            Self::ToolModuleCreated => "tool_module_created",
            Self::ToolModuleDeleted => "tool_module_deleted",
            Self::ToolInterlockCreated => "tool_interlock_created",
            Self::ToolInterlockDeleted => "tool_interlock_deleted",
            Self::ToolModuleSilent => "tool_module_silent",
            Self::ToolModuleReturned => "tool_module_returned",
            Self::UnauthorizedPowerDetected => "unauthorized_power_detected",
            Self::MqttBrokerLost => "mqtt_broker_lost",
            Self::MqttBrokerRestored => "mqtt_broker_restored",
            Self::EdgeIsolationReported => "edge_isolation_reported",
            Self::PowerCircuitCreated => "power_circuit_created",
            Self::PowerCircuitUpdated => "power_circuit_updated",
            Self::PowerCircuitDeleted => "power_circuit_deleted",
            Self::PowerOutletCreated => "power_outlet_created",
            Self::PowerOutletUpdated => "power_outlet_updated",
            Self::PowerOutletDeleted => "power_outlet_deleted",
            Self::PowerReceptacleCreated => "power_receptacle_created",
            Self::PowerReceptacleUpdated => "power_receptacle_updated",
            Self::PowerReceptacleDeleted => "power_receptacle_deleted",
            Self::CircuitOverageShutoff => "circuit_overage_shutoff",
            Self::EmergencyLockoutEngaged => "emergency_lockout_engaged",
            Self::EmergencyLockoutCleared => "emergency_lockout_cleared",
            Self::FirmwareSelftripReported => "firmware_selftrip_reported",
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
            Self::MembershipGranted => "membership_granted",
            Self::MembershipRevoked => "membership_revoked",
            Self::MembershipPaymentRecorded => "membership_payment_recorded",
            Self::MembershipLastAdminProtected => "membership_last_admin_protected",
            Self::SubscriptionStarted => "subscription_started",
            Self::SubscriptionCanceled => "subscription_canceled",
            Self::SubscriptionPaymentFailed => "subscription_payment_failed",
            Self::ToolUsageCharged => "tool_usage_charged",
            Self::ToolSessionAbandoned => "tool_session_abandoned",
            Self::ToolRateTierCreated => "tool_rate_tier_created",
            Self::ToolRateTierUpdated => "tool_rate_tier_updated",
            Self::ToolRateTierDeleted => "tool_rate_tier_deleted",
            Self::ToolTierAssigned => "tool_tier_assigned",
            Self::ToolTierUnassigned => "tool_tier_unassigned",
            Self::RoleCreated => "role_created",
            Self::RoleUpdated => "role_updated",
            Self::RoleDeleted => "role_deleted",
            Self::RolePermissionsChanged => "role_permissions_changed",
            Self::RoleInheritanceChanged => "role_inheritance_changed",
            Self::UserRoleAssigned => "user_role_assigned",
            Self::UserRoleUnassigned => "user_role_unassigned",
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
            TrainingWaiverGranted,
            TrainingWaiverRevoked,
            TrainingDocumentationAcknowledged,
            ToolAccessGranted,
            ToolAccessDenied,
            ToolActivated,
            ToolDeactivated,
            ToolUsageLogged,
            RevokedCardPresented,
            CardIssued,
            CardDisabled,
            CardReleased,
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
            ToolModuleCreated,
            ToolModuleDeleted,
            ToolInterlockCreated,
            ToolInterlockDeleted,
            ToolModuleSilent,
            ToolModuleReturned,
            UnauthorizedPowerDetected,
            MqttBrokerLost,
            MqttBrokerRestored,
            EdgeIsolationReported,
            PowerCircuitCreated,
            PowerCircuitUpdated,
            PowerCircuitDeleted,
            PowerOutletCreated,
            PowerOutletUpdated,
            PowerOutletDeleted,
            PowerReceptacleCreated,
            PowerReceptacleUpdated,
            PowerReceptacleDeleted,
            CircuitOverageShutoff,
            EmergencyLockoutEngaged,
            EmergencyLockoutCleared,
            FirmwareSelftripReported,
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
            MembershipGranted,
            MembershipRevoked,
            MembershipPaymentRecorded,
            MembershipLastAdminProtected,
            SubscriptionStarted,
            SubscriptionCanceled,
            SubscriptionPaymentFailed,
            ToolUsageCharged,
            ToolSessionAbandoned,
            ToolRateTierCreated,
            ToolRateTierUpdated,
            ToolRateTierDeleted,
            ToolTierAssigned,
            ToolTierUnassigned,
            RoleCreated,
            RoleUpdated,
            RoleDeleted,
            RolePermissionsChanged,
            RoleInheritanceChanged,
            UserRoleAssigned,
            UserRoleUnassigned,
        ]
    }
}

#[cfg(test)]
mod user_serialization_tests {
    use super::*;

    #[test]
    fn password_hash_is_never_serialized() {
        // #120/H9. `User` derives `Serialize` and is returned verbatim by the
        // training-roster endpoints, which bypass the hash-stripping
        // `UserResponse` DTO. The Argon2 hash must never reach a response body.
        // Mutation check: delete `#[serde(skip_serializing)]` from the field and
        // this fails on both the key's presence and the secret substring.
        let user = User {
            id: uuid::Uuid::nil(),
            username: "vector".into(),
            email: "vector@example.com".into(),
            password_hash: "$argon2id$SUPER-SECRET-HASH".into(),
            full_name: "Vector User".into(),
            is_active: true,
            created_at: chrono::DateTime::from_timestamp(0, 0)
                .expect("epoch")
                .naive_utc(),
            updated_at: chrono::DateTime::from_timestamp(0, 0)
                .expect("epoch")
                .naive_utc(),
            profile: serde_json::Value::Null,
            meta: serde_json::Value::Null,
            mfa_enrolled_at: None,
            email_verified_at: None,
            mailing_list_opt_out_at: None,
            membership_next_due_at: None,
            stripe_customer_id: None,
            stripe_subscription_id: None,
            subscription_status: None,
            token_version: 0,
        };

        let value = serde_json::to_value(&user).expect("User serializes");
        assert!(
            value.get("password_hash").is_none(),
            "password_hash must not appear in a serialized User: {value}"
        );
        let text = serde_json::to_string(&user).expect("User serializes");
        assert!(
            !text.contains("SUPER-SECRET-HASH"),
            "the password hash leaked into the response body: {text}"
        );
    }
}
