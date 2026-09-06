-- Retire the reconciliation and email-change event types.
--
-- The foreign key refuses these deletes if audit_logs still references them,
-- which is correct: those rows are the records the table exists to keep.
DELETE FROM audit_event_types WHERE name IN (
    'mailing_list_sync_add',
    'mailing_list_sync_remove',
    'user_email_change'
);
