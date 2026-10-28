-- Reverses the resource-level session record (#190).
--
-- Any room in use at the time loses its record: the coordinator's own session
-- carries on until a stop, and the platform forgets it was ever open. Nothing
-- about tools changes; their sessions never moved here.
DELETE FROM audit_event_types WHERE name IN ('resource_session_ended', 'resource_session_released_by_sweep');
DROP TABLE resource_sessions;
