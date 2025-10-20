-- Remove external_api_key column from tools table
DROP INDEX IF EXISTS idx_tools_external_api_key;
ALTER TABLE tools DROP COLUMN external_api_key;
