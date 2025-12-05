-- Add new training-related audit event types to the audit_logs table
-- This updates the CHECK constraint to include the new event types

-- First, drop the existing CHECK constraint
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

-- Add the new CHECK constraint with all event types (old + new training events)
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
    -- Tool-related events (pre-existing)
    'tool_access_granted',
    'tool_access_denied',
    'tool_activated',
    'tool_deactivated',
    'tool_usage_logged',
    -- New training-related events
    'training_session_started',
    'training_session_completed',
    'training_step_created',
    'training_step_updated', 
    'training_step_deleted',
    'trainer_assigned',
    'trainer_removed',
    'instructor_certified',
    'instructor_revoked'
));

-- Add comments for the new event types
COMMENT ON TABLE audit_logs IS 'Audit trail for all user-related operations, training activities, and security events';

-- Add an index specifically for training-related events for better query performance
CREATE INDEX IF NOT EXISTS idx_audit_logs_training_events 
ON audit_logs (event_type, created_at DESC) 
WHERE event_type LIKE 'training_%' OR event_type LIKE 'trainer_%' OR event_type LIKE 'instructor_%';