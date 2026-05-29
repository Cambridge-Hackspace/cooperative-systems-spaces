DROP TABLE IF EXISTS home_links;

ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check
CHECK (event_type IN (
    'user_registration','user_login','user_logout','user_role_change',
    'user_profile_update','user_password_change','user_activation',
    'user_deactivation','user_deletion','admin_config_reload','failed_login_attempt',
    'training_session_started','training_session_completed',
    'training_step_created','training_step_updated','training_step_deleted',
    'trainer_assigned','trainer_removed',
    'instructor_certified','instructor_revoked',
    'tool_access_granted','tool_access_denied',
    'tool_activated','tool_deactivated','tool_usage_logged',
    'device_invite_created','device_invite_used','device_invite_expired',
    'device_registered','device_name_changed','device_deleted','device_version_changed',
    'webhook_created','webhook_updated','webhook_deleted',
    'webhook_auth_header_created','webhook_auth_header_updated','webhook_auth_header_deleted',
    'mfa_totp_enrolled','mfa_totp_disabled',
    'mfa_webauthn_registered','mfa_webauthn_removed',
    'mfa_recovery_codes_regenerated','mfa_recovery_code_used',
    'mfa_login_passed','mfa_login_failed',
    'door_created','door_updated','door_deleted',
    'door_rule_added','door_rule_removed',
    'door_unlocked_card','door_unlocked_qr','door_unlocked_admin',
    'door_unlock_denied','door_checkin_recorded',
    'place_created','place_updated','place_moved','place_deleted',
    'schedule_created','schedule_updated','schedule_deleted'
));
