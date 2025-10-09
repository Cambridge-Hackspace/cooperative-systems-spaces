-- Add training_step_id to training_records table to make it step-based
ALTER TABLE training_records 
ADD COLUMN training_step_id UUID REFERENCES training_steps(id) ON DELETE CASCADE;

-- Create index for better query performance
CREATE INDEX idx_training_records_training_step_id ON training_records(training_step_id);
