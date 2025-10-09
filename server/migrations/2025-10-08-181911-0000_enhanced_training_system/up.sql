-- Enhanced Training System Migration
-- This migration creates the infrastructure for sequential tool training with prerequisites

-- First, create the training_status enum for tracking user progress
CREATE TYPE training_status AS ENUM (
    'not_started',
    'in_progress', 
    'completed',
    'failed',
    'expired'
);

-- Create assessment_type enum for different types of training assessments
CREATE TYPE assessment_type AS ENUM (
    'practical',
    'written',
    'both',
    'observation_only'
);

-- Sequential training steps for each tool
-- This replaces the simpler tool_training_types with a more detailed step-based system
CREATE TABLE training_steps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    step_number INTEGER NOT NULL, -- Order of this step in the training sequence
    step_name VARCHAR NOT NULL,
    description TEXT,
    training_materials_url TEXT, -- Link to documents, videos, resources
    requires_assessment BOOLEAN NOT NULL DEFAULT false,
    assessment_type assessment_type, -- Type of assessment if required
    duration_minutes INTEGER, -- Expected time to complete this step
    expires_after_days INTEGER, -- How long certification lasts (null = never expires)
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Ensure unique step numbers per tool
    UNIQUE(tool_id, step_number),
    
    -- Ensure step_number is positive
    CONSTRAINT positive_step_number CHECK (step_number > 0)
);

-- Prerequisites between training steps
-- Allows complex training dependencies (step X requires steps Y and Z to be completed first)
CREATE TABLE training_prerequisites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    training_step_id UUID NOT NULL REFERENCES training_steps(id) ON DELETE CASCADE,
    prerequisite_step_id UUID NOT NULL REFERENCES training_steps(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Prevent self-referencing prerequisites
    CONSTRAINT no_self_prerequisite CHECK (training_step_id != prerequisite_step_id),
    
    -- Ensure unique prerequisite relationships
    UNIQUE(training_step_id, prerequisite_step_id)
);

-- Individual user progress through training steps
-- This is the main table that tracks where each user is in their training journey
CREATE TABLE user_training_progress (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    training_step_id UUID NOT NULL REFERENCES training_steps(id) ON DELETE CASCADE,
    status training_status NOT NULL DEFAULT 'not_started',
    instructor_id UUID REFERENCES users(id), -- Who conducted the training
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ, -- When this certification expires
    assessment_score INTEGER, -- Score if assessment was graded (0-100)
    notes TEXT, -- Training notes from instructor
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Ensure unique progress record per user per step
    UNIQUE(user_id, training_step_id),
    
    -- Ensure assessment scores are valid
    CONSTRAINT valid_assessment_score CHECK (assessment_score IS NULL OR (assessment_score >= 0 AND assessment_score <= 100)),
    
    -- Ensure logical date progression
    CONSTRAINT logical_dates CHECK (
        started_at IS NULL OR completed_at IS NULL OR completed_at >= started_at
    )
);

-- Certified instructors for specific training steps
-- This table defines who is authorized to conduct training for each step
CREATE TABLE training_instructors (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    training_step_id UUID NOT NULL REFERENCES training_steps(id) ON DELETE CASCADE,
    certified_by UUID NOT NULL REFERENCES users(id), -- Who certified this instructor
    certified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ, -- When instructor certification expires
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Ensure unique instructor certification per step
    UNIQUE(user_id, training_step_id)
);

-- Create indexes for performance
CREATE INDEX idx_training_steps_tool_id ON training_steps(tool_id);
CREATE INDEX idx_training_steps_step_number ON training_steps(tool_id, step_number);
CREATE INDEX idx_training_prerequisites_step ON training_prerequisites(training_step_id);
CREATE INDEX idx_training_prerequisites_prereq ON training_prerequisites(prerequisite_step_id);
CREATE INDEX idx_user_training_progress_user ON user_training_progress(user_id);
CREATE INDEX idx_user_training_progress_step ON user_training_progress(training_step_id);
CREATE INDEX idx_user_training_progress_status ON user_training_progress(user_id, status);
CREATE INDEX idx_training_instructors_user ON training_instructors(user_id);
CREATE INDEX idx_training_instructors_step ON training_instructors(training_step_id);

-- Add updated_at triggers for automatic timestamp management
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_training_steps_updated_at 
    BEFORE UPDATE ON training_steps 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_user_training_progress_updated_at 
    BEFORE UPDATE ON user_training_progress 
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Sample training steps for demonstration (laser cutter example)
-- This shows how a complex tool might have sequential training requirements
INSERT INTO training_steps (tool_id, step_number, step_name, description, requires_assessment, assessment_type, duration_minutes, expires_after_days, created_by) 
SELECT 
    t.id,
    1,
    'Laser Safety Fundamentals',
    'Learn basic laser safety principles, eye protection, ventilation requirements, and emergency procedures.',
    true,
    'written'::assessment_type,
    30,
    365,
    u.id
FROM tools t, users u 
WHERE t.name ILIKE '%laser%' 
  AND u.role = 'admin'::user_role
  AND t.requires_training = true
LIMIT 1;

INSERT INTO training_steps (tool_id, step_number, step_name, description, requires_assessment, assessment_type, duration_minutes, expires_after_days, created_by) 
SELECT 
    t.id,
    2,
    'Material Knowledge',
    'Understanding of materials that can and cannot be laser cut, thickness limits, and material-specific settings.',
    true,
    'both'::assessment_type,
    45,
    365,
    u.id
FROM tools t, users u 
WHERE t.name ILIKE '%laser%' 
  AND u.role = 'admin'::user_role
  AND t.requires_training = true
LIMIT 1;

INSERT INTO training_steps (tool_id, step_number, step_name, description, requires_assessment, assessment_type, duration_minutes, expires_after_days, created_by) 
SELECT 
    t.id,
    3,
    'Software Training',
    'Learn to use the laser cutting software, import designs, set cut parameters, and prepare files for cutting.',
    true,
    'practical'::assessment_type,
    60,
    180,
    u.id
FROM tools t, users u 
WHERE t.name ILIKE '%laser%' 
  AND u.role = 'admin'::user_role
  AND t.requires_training = true
LIMIT 1;

INSERT INTO training_steps (tool_id, step_number, step_name, description, requires_assessment, assessment_type, duration_minutes, created_by) 
SELECT 
    t.id,
    4,
    'Supervised Practice',
    'Hands-on practice session with instructor supervision. Complete a simple cutting project.',
    true,
    'practical'::assessment_type,
    90,
    u.id
FROM tools t, users u 
WHERE t.name ILIKE '%laser%' 
  AND u.role = 'admin'::user_role
  AND t.requires_training = true
LIMIT 1;

-- Set up prerequisites (each step requires the previous one)
INSERT INTO training_prerequisites (training_step_id, prerequisite_step_id)
SELECT 
    ts2.id as training_step_id,
    ts1.id as prerequisite_step_id
FROM training_steps ts1
JOIN training_steps ts2 ON ts1.tool_id = ts2.tool_id 
WHERE ts1.step_number = ts2.step_number - 1
  AND ts2.step_number > 1;

-- Add some comments for documentation
COMMENT ON TABLE training_steps IS 'Sequential training steps required for tool certification';
COMMENT ON TABLE training_prerequisites IS 'Defines which training steps must be completed before others';
COMMENT ON TABLE user_training_progress IS 'Tracks individual user progress through training sequences';
COMMENT ON TABLE training_instructors IS 'Defines who is certified to conduct training for each step';
COMMENT ON COLUMN training_steps.step_number IS 'Order of this step in the training sequence (1, 2, 3, etc.)';
COMMENT ON COLUMN user_training_progress.status IS 'Current status of user in this training step';
COMMENT ON COLUMN user_training_progress.expires_at IS 'When this training certification expires (null = never expires)';