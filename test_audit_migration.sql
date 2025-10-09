-- Test script to verify the training audit events migration worked correctly

-- Test inserting one of each new event type to verify they're accepted
INSERT INTO audit_logs (event_type, event_data) VALUES 
('training_session_started', '{"test": true}'),
('training_session_completed', '{"test": true}'),
('training_step_created', '{"test": true}'),
('training_step_updated', '{"test": true}'), 
('training_step_deleted', '{"test": true}'),
('trainer_assigned', '{"test": true}'),
('trainer_removed', '{"test": true}'),
('instructor_certified', '{"test": true}'),
('instructor_revoked', '{"test": true}');

-- Verify all records were inserted
SELECT event_type, created_at 
FROM audit_logs 
WHERE event_data @> '{"test": true}' 
ORDER BY event_type;

-- Test that the training events index exists and is being used
EXPLAIN (ANALYZE, BUFFERS) 
SELECT * FROM audit_logs 
WHERE event_type LIKE 'training_%' 
ORDER BY created_at DESC 
LIMIT 10;

-- Clean up test data
DELETE FROM audit_logs WHERE event_data @> '{"test": true}';