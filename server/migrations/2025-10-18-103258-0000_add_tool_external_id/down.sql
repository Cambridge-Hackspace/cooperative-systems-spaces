-- Remove external_id column from tools table
DROP INDEX IF EXISTS idx_tools_external_id;
ALTER TABLE tools DROP COLUMN IF EXISTS external_id;
