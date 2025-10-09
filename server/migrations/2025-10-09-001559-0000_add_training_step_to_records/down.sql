-- Remove training_step_id column and index
DROP INDEX IF EXISTS idx_training_records_training_step_id;
ALTER TABLE training_records DROP COLUMN IF EXISTS training_step_id;
