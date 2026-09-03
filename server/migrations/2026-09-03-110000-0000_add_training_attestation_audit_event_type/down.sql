-- Retire the self-service attestation event type.
--
-- The foreign key will refuse this delete if audit_logs still holds rows
-- referencing it, and that is correct: an attestation record is kept for legal
-- reasons, and destroying one to make a revert succeed would be the wrong
-- trade. A revert that stops here is reporting a real conflict.
DELETE FROM audit_event_types WHERE name IN (
    'training_documentation_acknowledged'
);
