-- Retire the six transactional-email event types.
--
-- The foreign key will refuse these deletes if audit_logs still holds rows
-- referencing them, and that is correct: those rows are the records the table
-- exists to keep, and destroying them to make a revert succeed would be the
-- wrong trade. A revert that stops here is reporting a real conflict.
DELETE FROM audit_event_types WHERE name IN (
    'password_reset_requested',
    'password_reset_completed',
    'password_reset_failed',
    'email_verification_sent',
    'email_verified',
    'email_send_failed'
);
