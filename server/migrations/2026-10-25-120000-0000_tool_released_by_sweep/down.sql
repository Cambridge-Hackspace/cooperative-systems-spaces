-- Reverses #173's event registration. Releases already recorded stay in the
-- audit log; the delete below fails if any do, which is the honest outcome.
DELETE FROM audit_event_types WHERE name = 'tool_released_by_sweep';
