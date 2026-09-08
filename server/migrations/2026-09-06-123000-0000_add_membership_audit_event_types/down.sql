-- Retire the membership-billing event types.
--
-- The foreign key refuses these deletes if audit_logs still references them, and
-- that is correct: those rows are the records the table exists to keep. A revert
-- that stops here is reporting a real conflict rather than destroying history.
DELETE FROM audit_event_types WHERE name IN (
    'membership_granted',
    'membership_revoked',
    'membership_payment_recorded',
    'membership_last_admin_protected',
    'subscription_started',
    'subscription_canceled',
    'subscription_payment_failed'
);
