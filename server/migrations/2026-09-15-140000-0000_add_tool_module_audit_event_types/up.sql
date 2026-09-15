-- Audit event types for tool module bindings and interlock rules (#83).
-- Who wired a reader to a plug, and who authored or removed a safety interlock,
-- is exactly the sort of change that has to be reconstructable afterwards.
INSERT INTO audit_event_types (name) VALUES
    ('tool_module_created'),
    ('tool_module_deleted'),
    ('tool_interlock_created'),
    ('tool_interlock_deleted');
