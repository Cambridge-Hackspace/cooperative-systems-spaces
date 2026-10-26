DELETE FROM audit_event_types WHERE name = 'resource_updated';
ALTER TABLE resources DROP COLUMN session_profile;
