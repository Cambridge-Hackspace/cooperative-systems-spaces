DELETE FROM audit_event_types WHERE name IN (
    'tool_module_silent',
    'tool_module_returned',
    'unauthorized_power_detected',
    'mqtt_broker_lost',
    'mqtt_broker_restored',
    'edge_isolation_reported'
);
