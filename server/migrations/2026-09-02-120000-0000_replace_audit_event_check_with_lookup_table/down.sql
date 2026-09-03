-- Restore the CHECK constraint and drop the lookup table.
--
-- The list below is the sixty-eight values this migration's up.sql seeded, so
-- reverting lands exactly on the schema that preceded it.
--
-- Reverting is expected to FAIL, loudly, if a later migration added an event
-- type and rows were written using it. Diesel reverts in reverse order, and a
-- later migration's own down.sql deletes its row from audit_event_types but
-- cannot delete the audit_logs rows that reference it -- nor should it, since
-- those are the records this table exists to keep. A revert that stops there
-- is reporting a real conflict rather than hiding one.
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_fkey;

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

DROP TABLE audit_event_types;
