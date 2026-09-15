DELETE FROM audit_event_types WHERE name IN (
    'tool_module_created',
    'tool_module_deleted',
    'tool_interlock_created',
    'tool_interlock_deleted'
);
