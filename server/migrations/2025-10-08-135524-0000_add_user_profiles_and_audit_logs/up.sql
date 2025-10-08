-- Add profile column to users table
ALTER TABLE users 
ADD COLUMN profile JSONB DEFAULT '{}' NOT NULL;

-- Add index on profile for performance
CREATE INDEX idx_users_profile_gin ON users USING GIN (profile);

-- Create audit_logs table for tracking user-related changes  
-- Using TEXT for event_type and ip_address for simplicity

CREATE TABLE audit_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type TEXT NOT NULL CHECK (event_type IN (
        'user_registration', 'user_login', 'user_logout', 'user_role_change',
        'user_profile_update', 'user_password_change', 'user_activation',
        'user_deactivation', 'user_deletion', 'admin_config_reload',
        'failed_login_attempt'
    )),
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL, -- Who performed the action
    event_data JSONB DEFAULT '{}' NOT NULL,
    ip_address TEXT,
    user_agent TEXT,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP NOT NULL
);

-- Indexes for audit logs for performance
CREATE INDEX idx_audit_logs_user_id ON audit_logs (user_id);
CREATE INDEX idx_audit_logs_actor_id ON audit_logs (actor_id);
CREATE INDEX idx_audit_logs_event_type ON audit_logs (event_type);
CREATE INDEX idx_audit_logs_created_at ON audit_logs (created_at DESC);
CREATE INDEX idx_audit_logs_event_data_gin ON audit_logs USING GIN (event_data);

-- Comment the tables
COMMENT ON COLUMN users.profile IS 'User profile data stored as JSONB, structure defined by configuration';
COMMENT ON TABLE audit_logs IS 'Audit trail for all user-related operations and security events';
COMMENT ON COLUMN audit_logs.user_id IS 'User that was affected by the event (nullable for system events)';
COMMENT ON COLUMN audit_logs.actor_id IS 'User who performed the action (nullable for automated events)';
COMMENT ON COLUMN audit_logs.event_data IS 'Additional event-specific data in JSONB format';
