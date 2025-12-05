-- Rollback: Remove meta field from users table
ALTER TABLE users DROP COLUMN meta;
