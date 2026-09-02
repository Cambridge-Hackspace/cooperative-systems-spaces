-- Replace the audit_logs event-type CHECK constraint with a lookup table.
--
-- The constraint had been redefined eleven times, and every redefinition
-- restated the whole list -- sixty-eight values by the end. That shape has two
-- costs and both have already been paid once.
--
-- The first is that only the lexicographically last migration to restate the
-- constraint describes the live schema. Two branches can each add their own
-- event type, merge with no textual conflict at all, and produce a database
-- that silently forbids one of them. Every audit write in this codebase is
-- `let _ = create_audit_log(..)`, so such a row is never written and nothing
-- anywhere reports it.
--
-- The second is transcription: adding one event type meant reproducing
-- sixty-eight string literals correctly, in two files, by hand.
--
-- A lookup table costs a foreign-key check per insert and makes both failures
-- structurally impossible. A migration that adds an event type inserts one
-- row, touches no text another branch touches, and the live set becomes the
-- union of everything ever inserted rather than whatever the last author
-- managed to retype.

CREATE TABLE audit_event_types (
    name TEXT PRIMARY KEY
);

-- Seeded from the constraint's own list, generated out of it rather than
-- retyped. This is the last time these appear as a block.
INSERT INTO audit_event_types (name) VALUES
    -- User
    ('user_registration'),('user_login'),('user_logout'),('user_role_change'),
    ('user_profile_update'),('user_password_change'),('user_activation'),
    ('user_deactivation'),('user_deletion'),('admin_config_reload'),('failed_login_attempt'),
    -- Training
    ('training_session_started'),('training_session_completed'),
    ('training_step_created'),('training_step_updated'),('training_step_deleted'),
    ('trainer_assigned'),('trainer_removed'),
    ('instructor_certified'),('instructor_revoked'),
    -- Tool
    ('tool_access_granted'),('tool_access_denied'),
    ('tool_activated'),('tool_deactivated'),('tool_usage_logged'),
    -- Device
    ('device_invite_created'),('device_invite_used'),('device_invite_expired'),
    ('device_registered'),('device_name_changed'),('device_deleted'),('device_version_changed'),
    -- Webhook
    ('webhook_created'),('webhook_updated'),('webhook_deleted'),
    ('webhook_auth_header_created'),('webhook_auth_header_updated'),('webhook_auth_header_deleted'),
    -- MFA
    ('mfa_totp_enrolled'),('mfa_totp_disabled'),
    ('mfa_webauthn_registered'),('mfa_webauthn_removed'),
    ('mfa_recovery_codes_regenerated'),('mfa_recovery_code_used'),
    ('mfa_login_passed'),('mfa_login_failed'),
    -- Door
    ('door_created'),('door_updated'),('door_deleted'),
    ('door_rule_added'),('door_rule_removed'),
    ('door_unlocked_card'),('door_unlocked_qr'),('door_unlocked_admin'),
    ('door_unlock_denied'),('door_checkin_recorded'),
    -- Place
    ('place_created'),('place_updated'),('place_moved'),('place_deleted'),
    -- Schedule
    ('schedule_created'),('schedule_updated'),('schedule_deleted'),
    -- Home link
    ('home_link_created'),('home_link_updated'),('home_link_deleted'),
    -- Profile config (split out of admin_config_reload)
    ('profile_config_updated'),('profile_config_rolled_back');

ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

-- Validating the foreign key against existing rows is safe by induction: each
-- of the eleven restatements did DROP then ADD, and ADD validates every row.
-- Any deployment that reached this migration therefore holds no event_type
-- outside the list above, because a migration that dropped a value still in
-- use would have failed at deploy time rather than silently orphaning it.
ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_fkey
    FOREIGN KEY (event_type) REFERENCES audit_event_types (name);
