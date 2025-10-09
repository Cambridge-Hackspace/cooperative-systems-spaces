-- Trainer Assignment System Migration
-- This migration adds the ability for admins/staff to assign users as trainers for specific tools
-- and allows trainers to record training completion events

-- Extend existing tool_trainers table with additional columns
ALTER TABLE tool_trainers 
ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ,
ADD COLUMN IF NOT EXISTS is_active BOOLEAN NOT NULL DEFAULT true,
ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- Add constraint for logical date progression
DO $$ 
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.table_constraints 
        WHERE constraint_name = 'logical_assignment_dates' 
        AND table_name = 'tool_trainers'
    ) THEN
        ALTER TABLE tool_trainers 
        ADD CONSTRAINT logical_assignment_dates CHECK (
            expires_at IS NULL OR expires_at > authorized_at
        );
    END IF;
END $$;

-- Training records table - records when trainers complete training with users
CREATE TABLE IF NOT EXISTS training_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    trainee_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE, -- User being trained
    trainer_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE, -- User doing the training
    training_date DATE NOT NULL, -- Date the training occurred
    completion_status VARCHAR NOT NULL CHECK (completion_status IN ('completed', 'partial', 'failed')),
    hours_trained DECIMAL(4,2), -- Hours spent in training session
    skills_covered TEXT[], -- Array of skills/topics covered in this session
    notes TEXT, -- Training notes from the trainer
    next_steps TEXT, -- Recommended next steps for trainee
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Ensure training date is not in the future
    CONSTRAINT training_date_not_future CHECK (training_date <= CURRENT_DATE)
);

-- Create indexes for performance
CREATE INDEX IF NOT EXISTS idx_tool_trainers_active ON tool_trainers(tool_id, is_active) WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_training_records_tool_id ON training_records(tool_id);
CREATE INDEX IF NOT EXISTS idx_training_records_trainee ON training_records(trainee_user_id);
CREATE INDEX IF NOT EXISTS idx_training_records_trainer ON training_records(trainer_user_id);
CREATE INDEX IF NOT EXISTS idx_training_records_date ON training_records(training_date);

-- Add updated_at triggers
DROP TRIGGER IF EXISTS update_tool_trainers_updated_at ON tool_trainers;
CREATE TRIGGER update_tool_trainers_updated_at 
    BEFORE UPDATE ON tool_trainers 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

DROP TRIGGER IF EXISTS update_training_records_updated_at ON training_records;
CREATE TRIGGER update_training_records_updated_at 
    BEFORE UPDATE ON training_records 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Add comments for documentation
COMMENT ON TABLE tool_trainers IS 'Assigns users as authorized trainers for specific tools';
COMMENT ON TABLE training_records IS 'Records training sessions conducted by trainers';
COMMENT ON COLUMN tool_trainers.authorized_by IS 'Admin or staff member who assigned this trainer';
COMMENT ON COLUMN tool_trainers.expires_at IS 'When trainer authorization expires (null = never)';
COMMENT ON COLUMN training_records.trainee_user_id IS 'User receiving the training';
COMMENT ON COLUMN training_records.trainer_user_id IS 'User conducting the training (must be assigned trainer for this tool)';
COMMENT ON COLUMN training_records.completion_status IS 'Whether training was completed, partially completed, or failed';
COMMENT ON COLUMN training_records.skills_covered IS 'Array of specific skills or topics covered in this training session';
