DELETE FROM audit_event_types WHERE name IN ('member_notified', 'notification_kinds_declared');
DROP TABLE notification_messages;
DROP TABLE notification_preferences;
DROP TABLE notification_kinds;
