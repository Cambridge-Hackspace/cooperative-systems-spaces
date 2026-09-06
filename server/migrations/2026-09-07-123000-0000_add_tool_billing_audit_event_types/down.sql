-- The foreign key refuses these deletes if audit_logs still references them; that
-- is correct -- those rows are the records the table exists to keep.
DELETE FROM audit_event_types WHERE name IN (
    'tool_usage_charged',
    'tool_session_abandoned'
);
