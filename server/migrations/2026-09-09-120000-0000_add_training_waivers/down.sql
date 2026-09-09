DELETE FROM audit_event_types
 WHERE name IN ('training_waiver_granted', 'training_waiver_revoked');

DROP TABLE IF EXISTS training_waivers;
