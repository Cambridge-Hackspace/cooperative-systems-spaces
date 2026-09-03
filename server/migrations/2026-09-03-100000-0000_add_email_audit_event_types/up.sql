-- Six event types for transactional email and account recovery.
--
-- Six rows, and nothing else. Until
-- 2026-09-02-120000-0000_replace_audit_event_check_with_lookup_table this file
-- restated the entire seventy-four-value CHECK constraint, because the check
-- was redefined rather than appended to and only the last migration to restate
-- it described the live schema. This branch and the self-service attestation
-- branch each added an event type that way, merged without any textual
-- conflict, and whichever landed second would have silently dropped its own
-- value -- every audit write discards its Result, so the row would simply never
-- have been written. That is issue #18, and the lookup table is the fix.
--
-- Dated after that migration because these rows cannot be inserted before the
-- table exists. If this ever runs first the migration fails outright, which is
-- the point: the failure is loud now rather than a missing audit record.
INSERT INTO audit_event_types (name) VALUES
    ('password_reset_requested'),
    ('password_reset_completed'),
    ('password_reset_failed'),
    ('email_verification_sent'),
    ('email_verified'),
    ('email_send_failed');
