-- Retire the two mailing-list opt-in/opt-out event types.
--
-- The foreign key refuses these deletes if audit_logs still references them, and
-- that is correct: those rows are the records the table exists to keep. A revert
-- that stops here is reporting a real conflict rather than destroying history.
DELETE FROM audit_event_types WHERE name IN (
    'mailing_list_subscribe',
    'mailing_list_unsubscribe'
);
