-- One event type for self-service safety-documentation attestation.
--
-- One row, and nothing else. This registration used to live in
-- 2026-08-31-120000-0000_add_self_service_training_attestation as a restatement
-- of the entire audit_logs event-type CHECK constraint, because the check was
-- redefined rather than appended to and only the last migration to restate it
-- described the live schema. That is issue #18: this branch and the
-- transactional-email branch each added an event type that way, merged with no
-- textual conflict at all, and whichever landed second would have silently
-- dropped its own value. Every audit write discards its Result, so the row
-- would simply never have been written and nothing would have said so.
--
-- Split into its own migration rather than renamed wholesale: the two column
-- changes in the original do not depend on the lookup table and keep their own
-- date, while this row cannot be inserted before the table exists. If it ever
-- runs first the migration fails outright, which is the point -- the failure is
-- loud now rather than a missing audit record.
INSERT INTO audit_event_types (name) VALUES
    ('training_documentation_acknowledged');
