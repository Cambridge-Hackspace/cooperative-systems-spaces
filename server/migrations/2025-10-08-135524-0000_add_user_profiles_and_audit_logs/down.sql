-- Drop audit logs table and related objects
DROP TABLE IF EXISTS audit_logs;

-- Remove profile column and index from users table
DROP INDEX IF EXISTS idx_users_profile_gin;
ALTER TABLE users DROP COLUMN IF EXISTS profile;
