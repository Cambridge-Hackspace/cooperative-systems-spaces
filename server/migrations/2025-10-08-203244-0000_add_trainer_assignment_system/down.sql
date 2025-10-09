-- Reverse migration for trainer assignment system

-- Drop foreign key constraint first
ALTER TABLE training_records DROP CONSTRAINT IF EXISTS fk_trainer_must_be_assigned;

-- Drop triggers
DROP TRIGGER IF EXISTS update_training_records_updated_at ON training_records;
DROP TRIGGER IF EXISTS update_tool_trainers_updated_at ON tool_trainers;

-- Drop indexes
DROP INDEX IF EXISTS idx_training_records_date;
DROP INDEX IF EXISTS idx_training_records_trainer;
DROP INDEX IF EXISTS idx_training_records_trainee;
DROP INDEX IF EXISTS idx_training_records_tool_id;
DROP INDEX IF EXISTS idx_tool_trainers_active;
DROP INDEX IF EXISTS idx_tool_trainers_user_id;
DROP INDEX IF EXISTS idx_tool_trainers_tool_id;

-- Drop tables
DROP TABLE IF EXISTS training_records;
DROP TABLE IF EXISTS tool_trainers;
