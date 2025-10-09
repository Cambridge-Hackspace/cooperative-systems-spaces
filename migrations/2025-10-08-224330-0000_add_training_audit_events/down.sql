-- Rollback the training audit events migration
-- This reverts the audit_logs table to only support the original event types

-- Drop the training-specific index
DROP INDEX IF EXISTS idx_audit_logs_training_events;

-- Drop the current CHECK constraint
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

-- Restore the original CHECK constraint with only the original event types
ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check 
CHECK (event_type IN (
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
    'failed_login_attempt'
));

-- Restore the original comment
COMMENT ON TABLE audit_logs IS 'Audit trail for all user-related operations and security events';

-- Note: This migration will fail if there are any existing audit log records 
-- with the new training event types. In that case, those records would need to 
-- be manually cleaned up or migrated before running this rollback.