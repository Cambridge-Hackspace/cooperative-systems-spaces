-- Retire the cmi5 event types. The foreign key from audit_logs will refuse the
-- delete if any cmi5 audit rows still reference these names, which is correct:
-- an access-grant record (cmi5_au_satisfied) is evidence and should not be
-- destroyed to make a revert succeed. A revert that stops here reports a real
-- conflict.
DELETE FROM audit_event_types WHERE name IN (
    'cmi5_course_published',
    'cmi5_course_deleted',
    'cmi5_au_assigned_to_tool',
    'cmi5_launched',
    'cmi5_au_satisfied',
    'cmi5_course_exported'
);
