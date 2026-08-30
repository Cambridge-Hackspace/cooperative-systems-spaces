//! The route table the contract tier asserts against.
//!
//! **Hand-maintained, deliberately.** It is a committed statement of what the
//! API surface ought to be, kept separate from the router that implements it.
//! A table derived from the router at test time would agree with the router no
//! matter what the router said — delete a route by accident and it would vanish
//! from both sides at once, and the suite would stay green.
//!
//! What keeps it honest is the pairing: `checks/tests/route_table_matches.rs`
//! derives the inventory from `server/src/api/**` and asserts the two sets are
//! equal. That check may *report* drift; it may never absorb it. Adding a route
//! without adding a row here fails; removing one without removing its row
//! fails.
//!
//! Path parameters are substituted with a fixed UUID that matches nothing. That
//! is correct for this tier: every case here is rejected before the parameter
//! is ever looked up, and a row that got as far as needing a real row in the
//! database belongs to the live-database tier instead.

/// How a route is allowed to authenticate its caller.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Guard {
    Admin,
    Staff,
    Member,
    Auth,
    Device,
    /// Authenticates inside the handler from a bare `HeaderMap` rather than via
    /// an extractor — the ToolGuard device endpoints. Behaves as guarded; the
    /// divergence is recorded by `checks/tests/toolguard_auth.rs`.
    InlineAuth,
    /// Reachable without a credential by design.
    Public,
}

pub struct R(pub &'static str, pub &'static str, pub Guard);

impl R {
    pub fn method(&self) -> &'static str {
        self.0
    }
    pub fn path(&self) -> &'static str {
        self.1
    }
    pub fn guard(&self) -> Guard {
        self.2
    }
    /// True when the route must refuse a request carrying no valid credential.
    pub fn is_guarded(&self) -> bool {
        !matches!(self.guard(), Guard::Public)
    }
}

/// Every method × path the API router registers.
pub const ROUTES: &[R] = &[
    R("GET", "/api/admin/audit-logs", Guard::Admin), // admin::get_audit_logs
    R("GET", "/api/admin/devices", Guard::Admin), // devices::list_devices
    R("DELETE", "/api/admin/devices/00000000-0000-4000-8000-000000000001", Guard::Admin), // devices::delete_device
    R("PATCH", "/api/admin/devices/00000000-0000-4000-8000-000000000001/name", Guard::Admin), // devices::rename_device
    R("PATCH", "/api/admin/devices/00000000-0000-4000-8000-000000000001/place", Guard::Admin), // devices::set_device_place
    R("POST", "/api/admin/devices/invite", Guard::Admin), // devices::create_device_invite
    R("GET", "/api/admin/devices/invites", Guard::Admin), // devices::list_device_invites
    R("DELETE", "/api/admin/devices/invites/00000000-0000-4000-8000-000000000001", Guard::Admin), // devices::expire_device_invite
    R("GET", "/api/admin/doors", Guard::Admin), // doors::list_doors
    R("POST", "/api/admin/doors", Guard::Admin), // doors::create_door
    R("DELETE", "/api/admin/doors/00000000-0000-4000-8000-000000000001", Guard::Admin), // doors::delete_door
    R("GET", "/api/admin/doors/00000000-0000-4000-8000-000000000001", Guard::Admin), // doors::get_door
    R("PATCH", "/api/admin/doors/00000000-0000-4000-8000-000000000001", Guard::Admin), // doors::update_door
    R("GET", "/api/admin/doors/00000000-0000-4000-8000-000000000001/events", Guard::Admin), // doors::list_events
    R("GET", "/api/admin/doors/00000000-0000-4000-8000-000000000001/qr", Guard::Admin), // doors::get_qr_url
    R("POST", "/api/admin/doors/00000000-0000-4000-8000-000000000001/republish", Guard::Admin), // doors::admin_republish
    R("GET", "/api/admin/doors/00000000-0000-4000-8000-000000000001/rules", Guard::Admin), // doors::list_rules
    R("POST", "/api/admin/doors/00000000-0000-4000-8000-000000000001/rules", Guard::Admin), // doors::add_rule
    R("DELETE", "/api/admin/doors/00000000-0000-4000-8000-000000000001/rules/00000000-0000-4000-8000-000000000001", Guard::Admin), // doors::remove_rule
    R("POST", "/api/admin/doors/00000000-0000-4000-8000-000000000001/unlock", Guard::Admin), // doors::admin_unlock
    R("GET", "/api/admin/home-links", Guard::Admin), // home_links::list_links_admin
    R("POST", "/api/admin/home-links", Guard::Admin), // home_links::create_link
    R("DELETE", "/api/admin/home-links/00000000-0000-4000-8000-000000000001", Guard::Admin), // home_links::delete_link
    R("GET", "/api/admin/home-links/00000000-0000-4000-8000-000000000001", Guard::Admin), // home_links::get_link_admin
    R("PATCH", "/api/admin/home-links/00000000-0000-4000-8000-000000000001", Guard::Admin), // home_links::update_link
    R("POST", "/api/admin/pages/site/refresh", Guard::Admin), // admin::refresh_site_pages
    R("POST", "/api/admin/pages/wiki/refresh", Guard::Admin), // admin::refresh_wiki_pages
    R("GET", "/api/admin/places", Guard::Admin), // places::list_places_admin
    R("POST", "/api/admin/places", Guard::Admin), // places::create_place
    R("DELETE", "/api/admin/places/00000000-0000-4000-8000-000000000001", Guard::Admin), // places::delete_place
    R("GET", "/api/admin/places/00000000-0000-4000-8000-000000000001", Guard::Admin), // places::get_place_admin
    R("PATCH", "/api/admin/places/00000000-0000-4000-8000-000000000001", Guard::Admin), // places::update_place
    R("GET", "/api/admin/places/config", Guard::Admin), // places::get_config
    R("POST", "/api/admin/reload-config", Guard::Admin), // admin::reload_config
    R("GET", "/api/admin/roster", Guard::Admin), // admin::get_roster
    R("GET", "/api/admin/schedules", Guard::Admin), // schedules::list_schedules_admin
    R("POST", "/api/admin/schedules", Guard::Admin), // schedules::create_schedule
    R("DELETE", "/api/admin/schedules/00000000-0000-4000-8000-000000000001", Guard::Admin), // schedules::delete_schedule
    R("GET", "/api/admin/schedules/00000000-0000-4000-8000-000000000001", Guard::Admin), // schedules::get_schedule_admin
    R("PATCH", "/api/admin/schedules/00000000-0000-4000-8000-000000000001", Guard::Admin), // schedules::update_schedule
    R("PUT", "/api/admin/users/00000000-0000-4000-8000-000000000001/activate", Guard::Admin), // admin::activate_user
    R("PUT", "/api/admin/users/00000000-0000-4000-8000-000000000001/deactivate", Guard::Admin), // admin::deactivate_user
    R("DELETE", "/api/admin/users/00000000-0000-4000-8000-000000000001/mfa", Guard::Admin), // admin::reset_user_mfa
    R("PUT", "/api/admin/users/00000000-0000-4000-8000-000000000001/role", Guard::Admin), // admin::update_user_role
    R("GET", "/api/admin/webhooks", Guard::Admin), // webhooks::list_webhooks
    R("POST", "/api/admin/webhooks", Guard::Admin), // webhooks::create_webhook
    R("DELETE", "/api/admin/webhooks/00000000-0000-4000-8000-000000000001", Guard::Admin), // webhooks::delete_webhook
    R("GET", "/api/admin/webhooks/00000000-0000-4000-8000-000000000001", Guard::Admin), // webhooks::get_webhook
    R("PATCH", "/api/admin/webhooks/00000000-0000-4000-8000-000000000001", Guard::Admin), // webhooks::update_webhook
    R("POST", "/api/admin/webhooks/00000000-0000-4000-8000-000000000001/test", Guard::Admin), // webhooks::test_webhook
    R("GET", "/api/admin/webhooks/auth-headers", Guard::Admin), // webhooks::list_auth_headers
    R("POST", "/api/admin/webhooks/auth-headers", Guard::Admin), // webhooks::create_auth_header
    R("DELETE", "/api/admin/webhooks/auth-headers/00000000-0000-4000-8000-000000000001", Guard::Admin), // webhooks::delete_auth_header
    R("PATCH", "/api/admin/webhooks/auth-headers/00000000-0000-4000-8000-000000000001", Guard::Admin), // webhooks::update_auth_header
    R("GET", "/api/admin/webhooks/deliveries", Guard::Admin), // webhooks::list_deliveries
    R("GET", "/api/admin/webhooks/event-types", Guard::Admin), // webhooks::list_event_types
    R("POST", "/api/auth/login", Guard::Public), // auth::login
    R("POST", "/api/auth/logout", Guard::Public), // auth::logout
    R("GET", "/api/auth/me", Guard::Auth), // auth::me
    R("POST", "/api/auth/mfa/recovery-codes/regenerate", Guard::Auth), // mfa::recovery_regenerate
    R("GET", "/api/auth/mfa/status", Guard::Auth), // mfa::status
    R("DELETE", "/api/auth/mfa/totp", Guard::Auth), // mfa::totp_disable
    R("POST", "/api/auth/mfa/totp/confirm", Guard::Auth), // mfa::totp_confirm
    R("POST", "/api/auth/mfa/totp/setup", Guard::Auth), // mfa::totp_setup
    R("POST", "/api/auth/mfa/verify", Guard::Public), // mfa::verify_login
    R("GET", "/api/auth/mfa/webauthn", Guard::Auth), // mfa::webauthn_list
    R("DELETE", "/api/auth/mfa/webauthn/00000000-0000-4000-8000-000000000001", Guard::Auth), // mfa::webauthn_remove
    R("POST", "/api/auth/mfa/webauthn/register/begin", Guard::Auth), // mfa::webauthn_register_begin
    R("POST", "/api/auth/mfa/webauthn/register/finish", Guard::Auth), // mfa::webauthn_register_finish
    R("POST", "/api/auth/register", Guard::Public), // auth::register
    R("GET", "/api/calendar/events", Guard::Public), // calendar::get_calendar_events
    R("GET", "/api/calendar/events/refresh", Guard::Public), // calendar::refresh_calendar_events
    R("GET", "/api/config/public", Guard::Public), // config::get_public_config
    R("GET", "/api/config/registration", Guard::Public), // config::get_registration_config
    R("GET", "/api/config/tools", Guard::Public), // config::get_tools_config
    R("POST", "/api/devices/register", Guard::Public), // devices::register_device
    R("GET", "/api/devices/ws", Guard::Device), // devices::device_ws
    R("POST", "/api/doors/00000000-0000-4000-8000-000000000001/checkin", Guard::Auth), // doors::door_checkin
    R("GET", "/api/doors/00000000-0000-4000-8000-000000000001/info", Guard::Auth), // doors::door_info
    R("GET", "/api/instance/qr", Guard::Auth), // instance::get_instance_qr
    R("GET", "/api/pages/navigation", Guard::Public), // pages::get_navigation
    R("GET", "/api/pages/page", Guard::Public), // pages::list_site_pages
    R("GET", "/api/pages/page/index", Guard::Public), // pages::get_site_index
    R("GET", "/api/pages/page/some/slug", Guard::Public), // pages::get_site_page
    R("GET", "/api/pages/wiki", Guard::Public), // pages::list_wiki_pages
    R("GET", "/api/pages/wiki/some/slug", Guard::Public), // pages::get_wiki_page
    R("GET", "/api/places", Guard::Auth), // places::list_places_member
    R("GET", "/api/places/00000000-0000-4000-8000-000000000001", Guard::Auth), // places::get_place_member
    R("GET", "/api/profiles/00000000-0000-4000-8000-000000000001", Guard::Auth), // profiles::get_user_profile
    R("PUT", "/api/profiles/00000000-0000-4000-8000-000000000001", Guard::Auth), // profiles::update_user_profile
    R("GET", "/api/profiles/config", Guard::Auth), // profiles::get_profile_config
    R("PUT", "/api/profiles/config", Guard::Admin), // profiles::update_profile_config
    R("POST", "/api/profiles/config/rollback/00000000-0000-4000-8000-000000000001", Guard::Admin), // profiles::rollback_profile_config
    R("GET", "/api/profiles/config/versions", Guard::Admin), // profiles::list_profile_config_versions
    R("GET", "/api/public/home-links", Guard::Public), // home_links::list_links_public
    R("GET", "/api/public/schedules", Guard::Public), // schedules::list_schedules_public
    R("GET", "/api/schedules", Guard::Auth), // schedules::list_schedules_member
    R("GET", "/api/schedules/00000000-0000-4000-8000-000000000001", Guard::Auth), // schedules::get_schedule_member
    R("GET", "/api/toolguard", Guard::Public), // toolguard::api_status
    R("POST", "/api/toolguard/boot-reset", Guard::InlineAuth), // toolguard::boot_reset
    R("GET", "/api/toolguard/sync", Guard::InlineAuth), // toolguard::sync
    R("GET", "/api/toolguard/tool-log", Guard::InlineAuth), // toolguard::tool_log
    R("GET", "/api/toolguard/tool-off", Guard::InlineAuth), // toolguard::tool_off
    R("GET", "/api/toolguard/tool-on", Guard::InlineAuth), // toolguard::tool_on
    R("GET", "/api/tools", Guard::Staff), // tools::list_tools
    R("POST", "/api/tools", Guard::Staff), // tools::create_tool
    R("DELETE", "/api/tools/00000000-0000-4000-8000-000000000001", Guard::Staff), // tools::delete_tool
    R("GET", "/api/tools/00000000-0000-4000-8000-000000000001", Guard::Staff), // tools::get_tool
    R("PUT", "/api/tools/00000000-0000-4000-8000-000000000001", Guard::Staff), // tools::update_tool
    R("GET", "/api/tools/00000000-0000-4000-8000-000000000001/can-use", Guard::Auth), // tools::can_user_use_tool
    R("GET", "/api/tools/00000000-0000-4000-8000-000000000001/events", Guard::Staff), // tools::get_tool_events
    R("POST", "/api/tools/00000000-0000-4000-8000-000000000001/events", Guard::Staff), // tools::add_tool_event
    R("PUT", "/api/tools/00000000-0000-4000-8000-000000000001/status", Guard::Staff), // tools::change_tool_status
    R("GET", "/api/tools/00000000-0000-4000-8000-000000000001/trainers", Guard::Staff), // tools::get_tool_trainers
    R("POST", "/api/tools/00000000-0000-4000-8000-000000000001/trainers", Guard::Staff), // tools::authorize_trainer
    R("GET", "/api/tools/00000000-0000-4000-8000-000000000001/training-types", Guard::Staff), // tools::get_tool_training_types
    R("POST", "/api/tools/00000000-0000-4000-8000-000000000001/training-types", Guard::Staff), // tools::create_training_type
    R("GET", "/api/tools/00000000-0000-4000-8000-000000000001/user-training", Guard::Auth), // tools::get_user_training_for_tool
    R("GET", "/api/tools/available", Guard::Auth), // tools::list_available_tools
    R("GET", "/api/tools/user-training", Guard::Auth), // tools::get_user_training
    R("DELETE", "/api/tools/user-training/00000000-0000-4000-8000-000000000001", Guard::Staff), // tools::revoke_training
    R("POST", "/api/tools/user-training/00000000-0000-4000-8000-000000000001", Guard::Staff), // tools::complete_training
    R("GET", "/api/tools/visible", Guard::Auth), // tools::list_visible_tools
    R("GET", "/api/trainers/tools/00000000-0000-4000-8000-000000000001/trainers", Guard::Auth), // trainers::get_tool_trainers
    R("POST", "/api/trainers/tools/00000000-0000-4000-8000-000000000001/trainers", Guard::Staff), // trainers::assign_tool_trainer
    R("DELETE", "/api/trainers/tools/00000000-0000-4000-8000-000000000001/trainers/00000000-0000-4000-8000-000000000001", Guard::Staff), // trainers::remove_tool_trainer
    R("PUT", "/api/trainers/tools/00000000-0000-4000-8000-000000000001/trainers/00000000-0000-4000-8000-000000000001", Guard::Staff), // trainers::update_tool_trainer
    R("GET", "/api/trainers/tools/00000000-0000-4000-8000-000000000001/trainers/check/00000000-0000-4000-8000-000000000001", Guard::Auth), // trainers::check_trainer_authorization
    R("GET", "/api/trainers/training-records", Guard::Auth), // trainers::get_training_records
    R("POST", "/api/trainers/training-records", Guard::Auth), // trainers::create_training_record
    R("PUT", "/api/trainers/training-records/00000000-0000-4000-8000-000000000001", Guard::Auth), // trainers::update_training_record
    R("GET", "/api/trainers/users/00000000-0000-4000-8000-000000000001/training-records", Guard::Auth), // trainers::get_user_training_records
    R("GET", "/api/training/access/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::check_my_tool_access
    R("GET", "/api/training/access/00000000-0000-4000-8000-000000000001/00000000-0000-4000-8000-000000000001", Guard::Staff), // training::check_tool_access
    R("GET", "/api/training/history/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::get_training_history_for_tool
    R("GET", "/api/training/instructors", Guard::Auth), // training::get_instructors
    R("POST", "/api/training/instructors", Guard::Staff), // training::certify_instructor
    R("DELETE", "/api/training/instructors/00000000-0000-4000-8000-000000000001", Guard::Staff), // training::revoke_instructor_certification
    R("DELETE", "/api/training/prerequisites/00000000-0000-4000-8000-000000000001", Guard::Staff), // training::remove_prerequisite
    R("GET", "/api/training/progress", Guard::Auth), // training::get_user_training_progress
    R("GET", "/api/training/progress/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::get_user_training_progress_by_user
    R("GET", "/api/training/progress/00000000-0000-4000-8000-000000000001/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::get_specific_progress
    R("PUT", "/api/training/progress/00000000-0000-4000-8000-000000000001/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::update_training_progress
    R("GET", "/api/training/roster", Guard::Auth), // training::get_training_roster
    R("GET", "/api/training/roster/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::get_training_roster_for_tool
    // The session endpoints moved under /progress/{user_id}/ so an instructor
    // can start or complete a session for somebody else; the handlers now take
    // the subject from the path and check `can_access_staff` when it is not the
    // caller. The frontend was already calling these paths -- they were on the
    // route-parity unresolved list until the server caught up.
    R("POST", "/api/training/progress/00000000-0000-4000-8000-000000000001/complete", Guard::Auth), // training::complete_training_session
    R("POST", "/api/training/progress/00000000-0000-4000-8000-000000000001/start", Guard::Auth), // training::start_training_session
    R("GET", "/api/training/steps", Guard::Auth), // training::get_training_steps
    R("POST", "/api/training/steps", Guard::Staff), // training::create_training_step
    R("DELETE", "/api/training/steps/00000000-0000-4000-8000-000000000001", Guard::Staff), // training::delete_training_step
    R("GET", "/api/training/steps/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::get_training_step
    R("PUT", "/api/training/steps/00000000-0000-4000-8000-000000000001", Guard::Staff), // training::update_training_step
    R("PUT", "/api/training/steps/00000000-0000-4000-8000-000000000001/position", Guard::Staff), // training::update_training_step_position
    R("GET", "/api/training/steps/00000000-0000-4000-8000-000000000001/prerequisites", Guard::Auth), // training::get_prerequisites
    R("POST", "/api/training/steps/00000000-0000-4000-8000-000000000001/prerequisites", Guard::Staff), // training::add_prerequisite
    R("GET", "/api/training/tools/00000000-0000-4000-8000-000000000001/overview", Guard::Auth), // training::get_tool_training_overview
    R("GET", "/api/training/tools/00000000-0000-4000-8000-000000000001/overview/00000000-0000-4000-8000-000000000001", Guard::Auth), // training::get_user_tool_training_overview
    R("GET", "/api/training/tools/00000000-0000-4000-8000-000000000001/overview/me", Guard::Auth), // training::get_my_tool_training_overview
    R("GET", "/api/training/tools/00000000-0000-4000-8000-000000000001/steps", Guard::Auth), // training::get_tool_training_steps
    R("GET", "/api/users", Guard::Admin), // users::list_users
    R("DELETE", "/api/users/00000000-0000-4000-8000-000000000001", Guard::Admin), // users::delete_user
    R("GET", "/api/users/00000000-0000-4000-8000-000000000001", Guard::Auth), // users::get_user_by_id
    R("PUT", "/api/users/00000000-0000-4000-8000-000000000001", Guard::Auth), // users::update_user
    R("PUT", "/api/users/me/password", Guard::Auth), // users::change_own_password
    R("PATCH", "/api/users/00000000-0000-4000-8000-000000000001/theme", Guard::Auth), // users::update_user_theme
];
