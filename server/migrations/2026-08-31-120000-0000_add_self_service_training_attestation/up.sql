-- Self-service training attestation.
--
-- Issue #2 asks for a binding "I have read this safety documentation"
-- checkbox, so that there is a record. Two columns and one event type.
--
-- self_attestable marks a step whose completion is the trainee's own act
-- rather than a trainer's sign-off. Default false: every step that exists
-- today was signed off by staff or a certified instructor, and silently
-- turning any of them into something a member can grant themselves would
-- be the opposite of what this is for.
ALTER TABLE training_steps
    ADD COLUMN self_attestable BOOLEAN NOT NULL DEFAULT false;

-- What the member actually agreed to, captured when they agree to it.
--
-- training_steps.training_materials_url is mutable through PUT /steps/{id}.
-- Without a snapshot, editing the link -- or replacing the page behind it --
-- silently rewrites what every prior attestation was an attestation to, which
-- is the one property a record kept for legal reasons cannot afford to lose.
--
-- Nullable, no default: rows that predate this column were completed before
-- anything was captured, and inventing a URL for them would be worse than
-- admitting none was recorded.
ALTER TABLE user_training_progress
    ADD COLUMN acknowledged_materials_url TEXT;

-- Restated in full, per the pattern of the ten migrations that redefined this
-- constraint before it. Adding the type here and nowhere else is the accident
-- checks/tests/audit_event_types.rs exists to catch: every audit write in this
-- codebase discards its result, so a Rust variant the constraint does not list
-- produces no error anywhere -- the row is simply never written.
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
    'training_documentation_acknowledged',
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
