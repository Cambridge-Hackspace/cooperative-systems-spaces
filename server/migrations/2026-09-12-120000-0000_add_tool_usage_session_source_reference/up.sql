-- #38: an idempotency + provenance key for historical tool-usage sessions
-- imported from ToolPass. NULL for live sessions the app opens itself; set to
-- the source system's session id (e.g. "toolpass:session:<id>") for imports, so
-- the loader can be re-run without duplicating rows. Partial-unique so many
-- live NULLs coexist while each imported id stays unique.
ALTER TABLE tool_usage_sessions ADD COLUMN source_reference TEXT;

CREATE UNIQUE INDEX idx_tool_usage_sessions_source_reference
    ON tool_usage_sessions (source_reference)
    WHERE source_reference IS NOT NULL;
