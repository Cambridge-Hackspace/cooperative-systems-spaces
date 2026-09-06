-- Audit event types for metered tool billing. Inserted as rows into the
-- audit_event_types lookup (the idiom since the CHECK constraint was replaced by
-- a table), so a concurrent branch adding its own event types does not collide.
--
-- The raw usage report keeps the existing 'tool_usage_logged' event; these two
-- record the money outcome: a settled charge, and a session the sweep had to
-- close because it was never stopped. Payloads carry amounts/minutes, no card
-- data (there is none in this path).
INSERT INTO audit_event_types (name) VALUES
    ('tool_usage_charged'),
    ('tool_session_abandoned');
