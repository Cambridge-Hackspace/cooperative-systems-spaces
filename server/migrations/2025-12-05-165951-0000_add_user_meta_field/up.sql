-- Add meta field to users table for system data
ALTER TABLE users ADD COLUMN meta JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Add a comment to clarify the purpose
COMMENT ON COLUMN users.meta IS 'System-managed metadata (read-only for users). Use profile field for user-editable data.';
