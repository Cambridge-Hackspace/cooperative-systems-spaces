-- Add ToolPass-related audit event types to the audit_logs table
-- This updates the CHECK constraint to include the new event types

-- First, drop the existing CHECK constraint
ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_event_type_check;

-- Add the new CHECK constraint with all event types (old + new ToolPass events)
ALTER TABLE audit_logs 
ADD CONSTRAINT audit_logs_event_type_check 
CHECK (event_type IN (
    'user_registration', 'user_login', 'user_logout', 'user_role_change',
    'user_profile_update', 'user_password_change', 'user_activation',
    'user_deactivation', 'user_deletion', 'admin_config_reload',
    'failed_login_attempt',
    'training_session_started', 'training_session_completed',
    'training_step_created', 'training_step_updated', 'training_step_deleted',
    'trainer_assigned', 'trainer_removed',
    'instructor_certified', 'instructor_revoked',
    -- ToolPass/Tool usage events
    'tool_access_granted', 'tool_access_denied',
    'tool_activated', 'tool_deactivated', 'tool_usage_logged'
));

-- Add index specifically for ToolPass-related audit events
CREATE INDEX idx_audit_logs_toolpass_events ON audit_logs (event_type, created_at DESC) 
WHERE event_type IN (
    'tool_access_granted', 'tool_access_denied',
    'tool_activated', 'tool_deactivated', 'tool_usage_logged'
);
