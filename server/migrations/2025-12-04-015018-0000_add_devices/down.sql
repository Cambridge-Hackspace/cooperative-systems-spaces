-- Rollback device-related tables and enums

-- Drop indexes
DROP INDEX IF EXISTS idx_audit_logs_device_events;
DROP INDEX IF EXISTS idx_space_device_auth_requests_expires;
DROP INDEX IF EXISTS idx_space_device_auth_requests_code;
DROP INDEX IF EXISTS idx_space_device_auth_token;
DROP INDEX IF EXISTS idx_space_device_auth_device_id;
DROP INDEX IF EXISTS idx_space_devices_last_seen_at;
DROP INDEX IF EXISTS idx_space_devices_deleted_at;
DROP INDEX IF EXISTS idx_space_devices_kind;

-- Drop tables
DROP TABLE IF EXISTS space_device_auth_requests;
DROP TABLE IF EXISTS space_device_auth;
DROP TABLE IF EXISTS space_devices;

-- Drop enums
DROP TYPE IF EXISTS space_device_platform;
DROP TYPE IF EXISTS space_device_kind;

-- Restore previous audit_logs constraint (without device events)
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check 
CHECK (event_type IN (
    -- Original user-related events
    'user_registration', 
    'user_login', 
    'user_logout', 
    'user_role_change',
    'user_profile_update', 
    'user_password_change', 
    'user_activation',
    'user_deactivation', 
    'user_deletion', 
    'admin_config_reload',
    'failed_login_attempt',
    -- Training-related events
    'training_session_started',
    'training_session_completed',
    'training_step_created',
    'training_step_updated', 
    'training_step_deleted',
    'trainer_assigned',
    'trainer_removed',
    'instructor_certified',
    'instructor_revoked',
    -- Tool-related events
    'tool_access_granted',
    'tool_access_denied',
    'tool_activated',
    'tool_deactivated',
    'tool_usage_logged'
));
