-- Reverses #87's heartbeat. Emitted heartbeat events stay in the audit log.
DROP TABLE IF EXISTS alert_heartbeat_runs;
DELETE FROM audit_event_types WHERE name = 'alert_heartbeat';
