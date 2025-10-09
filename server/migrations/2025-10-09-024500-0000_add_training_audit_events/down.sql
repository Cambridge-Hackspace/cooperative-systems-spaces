-- Rollback migration for training audit event types

-- Drop the training events index
DROP INDEX IF EXISTS idx_audit_logs_training_events;

-- Drop the current CHECK constraint
ALTER TABLE audit_logs DROP CONSTRAINT audit_logs_event_type_check;

-- Restore the original CHECK constraint with only the original event types
ALTER TABLE audit_logs 
ADD CONSTRAINT audit_logs_event_type_check 
CHECK (event_type IN (
    'user_registration', 'user_login', 'user_logout', 'user_role_change',
    'user_profile_update', 'user_password_change', 'user_activation',
    'user_deactivation', 'user_deletion', 'admin_config_reload',
    'failed_login_attempt'
));