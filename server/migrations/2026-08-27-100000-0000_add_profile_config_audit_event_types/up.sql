-- The AuditEventType::ProfileConfigUpdated / ProfileConfigRolledBack
-- variants were added in Rust without updating this check constraint, so
-- every profile-config audit write has been silently failing (caught and
-- only logged, not surfaced to the API caller) since that change shipped.
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check
CHECK (event_type IN (
    -- User
    'user_registration','user_login','user_logout','user_role_change',
    'user_profile_update','user_password_change','user_activation',
    'user_deactivation','user_deletion','admin_config_reload','failed_login_attempt',
    -- Training
    'training_session_started','training_session_completed',
    'training_step_created','training_step_updated','training_step_deleted',
    'trainer_assigned','trainer_removed',
    'instructor_certified','instructor_revoked',
    -- Tool
    'tool_access_granted','tool_access_denied',
    'tool_activated','tool_deactivated','tool_usage_logged',
    -- Device
    'device_invite_created','device_invite_used','device_invite_expired',
    'device_registered','device_name_changed','device_deleted','device_version_changed',
    -- Webhook
    'webhook_created','webhook_updated','webhook_deleted',
    'webhook_auth_header_created','webhook_auth_header_updated','webhook_auth_header_deleted',
    -- MFA
    'mfa_totp_enrolled','mfa_totp_disabled',
    'mfa_webauthn_registered','mfa_webauthn_removed',
    'mfa_recovery_codes_regenerated','mfa_recovery_code_used',
    'mfa_login_passed','mfa_login_failed',
    -- Door
    'door_created','door_updated','door_deleted',
    'door_rule_added','door_rule_removed',
    'door_unlocked_card','door_unlocked_qr','door_unlocked_admin',
    'door_unlock_denied','door_checkin_recorded',
    -- Place
    'place_created','place_updated','place_moved','place_deleted',
    -- Schedule
    'schedule_created','schedule_updated','schedule_deleted',
    -- Home link
    'home_link_created','home_link_updated','home_link_deleted',
    -- Profile config (split out of admin_config_reload)
    'profile_config_updated','profile_config_rolled_back'
));
