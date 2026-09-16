-- Audit event types for tool bypass detection (#84).
--
-- These are the artefact the issue exists to produce: the record that survives
-- to be shown to an insurer. They are deliberately transition events rather than
-- level events -- a module that has been silent for a week should be one row
-- saying when it went quiet, not one row per sweep for a week. Nothing prunes
-- audit_logs, so a level-based signal here would grow without bound and bury
-- itself.
INSERT INTO audit_event_types (name) VALUES
    ('tool_module_silent'),
    ('tool_module_returned'),
    ('unauthorized_power_detected'),
    ('mqtt_broker_lost'),
    ('mqtt_broker_restored'),
    ('edge_isolation_reported');
