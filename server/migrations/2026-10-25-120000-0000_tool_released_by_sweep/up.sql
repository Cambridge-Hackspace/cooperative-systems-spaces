-- #173: the session sweep returns a tool nobody released to idle.
--
-- Nothing else moved a tool from in_use back to idle except a tool-off, an
-- administrator, or the site-wide boot-reset. The sweep that now does it
-- records each release as its own audit event, at Warning severity: the device
-- that opened the session did not close it, and will go on not closing it until
-- someone looks. Registered here because `audit_event_types` is the vocabulary
-- `audit_logs.event_type` is checked against, and an unregistered type is a
-- foreign-key violation that `create_audit_log`'s callers discard.

INSERT INTO audit_event_types (name) VALUES ('tool_released_by_sweep');
