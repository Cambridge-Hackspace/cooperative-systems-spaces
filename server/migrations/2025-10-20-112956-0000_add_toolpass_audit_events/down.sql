-- Revert ToolPass audit event types from the audit_logs table

-- Drop the ToolPass events index
DROP INDEX IF EXISTS idx_audit_logs_toolpass_events;

-- First, drop the existing CHECK constraint
ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_event_type_check;

-- Restore the previous CHECK constraint without ToolPass events
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
    'instructor_certified', 'instructor_revoked'
));
