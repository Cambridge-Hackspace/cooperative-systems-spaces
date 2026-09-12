DROP INDEX IF EXISTS idx_tool_usage_sessions_source_reference;
ALTER TABLE tool_usage_sessions DROP COLUMN IF EXISTS source_reference;
