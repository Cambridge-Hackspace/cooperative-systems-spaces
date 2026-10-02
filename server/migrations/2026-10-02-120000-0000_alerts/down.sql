-- Reverses #87's alerts. Acknowledgements are lost; the audit rows they
-- pointed at are untouched.
DROP TABLE IF EXISTS alert_acknowledgements;
DELETE FROM audit_event_types WHERE name = 'alert_acknowledged';
DELETE FROM role_permissions WHERE permission_key IN ('alerts.view', 'alerts.acknowledge');
DELETE FROM permissions WHERE key IN ('alerts.view', 'alerts.acknowledge');
