-- Rollback the Enhanced Training System Migration
-- This will remove the sequential training system and restore the original simpler structure

-- Drop tables in reverse order of dependencies
DROP TABLE IF EXISTS training_instructors CASCADE;
DROP TABLE IF EXISTS user_training_progress CASCADE; 
DROP TABLE IF EXISTS training_prerequisites CASCADE;
DROP TABLE IF EXISTS training_steps CASCADE;

-- Drop the custom types
DROP TYPE IF EXISTS training_status CASCADE;
DROP TYPE IF EXISTS assessment_type CASCADE;

-- Drop the trigger function
DROP FUNCTION IF EXISTS update_updated_at_column() CASCADE;

-- Note: We don't restore the original tool_training_types table here
-- as that would require preserving its data during the migration.
-- In a production environment, you would need a more sophisticated
-- rollback strategy that preserves existing training data.