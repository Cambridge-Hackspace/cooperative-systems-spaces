-- Audit event types for the cmi5 subsystem.
--
-- One row per event, inserted into the lookup table so the permitted set is the
-- order-independent union across all migrations (see
-- 2026-09-02-120000-0000_replace_audit_event_check_with_lookup_table). Kept in
-- its own migration, after the cmi5 tables, so it touches text no other branch
-- touches and cannot run before the lookup table exists.
--
-- cmi5_au_satisfied is the security-relevant one: it is the record that a
-- browser course led to a physical-tool-access grant, the cmi5 analogue of a
-- trainer sign-off's training_session_completed.
INSERT INTO audit_event_types (name) VALUES
    ('cmi5_course_published'),
    ('cmi5_course_deleted'),
    ('cmi5_au_assigned_to_tool'),
    ('cmi5_launched'),
    ('cmi5_au_satisfied'),
    ('cmi5_course_exported');
