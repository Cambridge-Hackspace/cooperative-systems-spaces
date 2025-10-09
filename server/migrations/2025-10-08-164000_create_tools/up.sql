-- Create custom enum for tool status
CREATE TYPE tool_status AS ENUM (
    'idle',
    'in_use',
    'maintenance',
    'broken',
    'repair',
    'retired'
);

-- Create custom enum for tool category (sawing, power tools, etc)
CREATE TYPE tool_category AS ENUM (
    'saw',
    'powertool',
    'hand_tools',
    'measuring',
    'safety',
    'electronics',
    'woodworking',
    'metalworking',
    '3d_printing',
    'laser_cutting',
    'welding',
    'other'
);

-- Tools table
CREATE TABLE tools (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR NOT NULL,
    description TEXT,
    category tool_category NOT NULL DEFAULT 'other',
    status tool_status NOT NULL DEFAULT 'idle',
    barcode VARCHAR UNIQUE,
    serial_number VARCHAR,
    location VARCHAR,
    purchase_date DATE,
    purchase_price DECIMAL(10,2),
    maintenance_notes TEXT,
    requires_training BOOLEAN NOT NULL DEFAULT false,
    created_by UUID NOT NULL REFERENCES users(id), -- Staff member who created tool
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Tool state change log - tracks all events/status changes
CREATE TABLE tool_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    event_type VARCHAR NOT NULL, -- 'status_change', 'maintenance', 'scan', 'checkout', 'return'
    old_status tool_status,
    new_status tool_status,
    user_id UUID REFERENCES users(id), -- User who triggered the event
    actor_id UUID REFERENCES users(id), -- Staff member who processed the event
    notes TEXT,
    scan_data JSONB, -- Store scan results and additional data
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Tool training definitions (what training is available for tools)
CREATE TABLE tool_training_types (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    training_name VARCHAR NOT NULL,
    description TEXT,
    expires_after_days INTEGER, -- NULL means training doesn't expire
    created_by UUID NOT NULL REFERENCES users(id), -- Staff who created this training
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- User training records (which users have completed which trainings)
CREATE TABLE user_tool_training (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    training_type_id UUID NOT NULL REFERENCES tool_training_types(id) ON DELETE CASCADE,
    trainer_id UUID NOT NULL REFERENCES users(id), -- User marked as trainer who provided training
    trained_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE, -- Calculated based on training type
    notes TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    UNIQUE(user_id, training_type_id) -- One training record per user per training type
);

-- Tool trainers (which users are authorized to train others on specific tools)
CREATE TABLE tool_trainers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    authorized_by UUID NOT NULL REFERENCES users(id), -- Staff who authorized this trainer
    authorized_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    notes TEXT,
    
    UNIQUE(user_id, tool_id) -- One trainer record per user per tool
);

-- Add indexes for better performance
CREATE INDEX idx_tools_status ON tools(status);
CREATE INDEX idx_tools_category ON tools(category);
CREATE INDEX idx_tools_requires_training ON tools(requires_training);

CREATE INDEX idx_tool_events_tool_id ON tool_events(tool_id);
CREATE INDEX idx_tool_events_created_at ON tool_events(created_at DESC);
CREATE INDEX idx_tool_events_event_type ON tool_events(event_type);

CREATE INDEX idx_tool_training_types_tool_id ON tool_training_types(tool_id);

CREATE INDEX idx_user_tool_training_user_id ON user_tool_training(user_id);
CREATE INDEX idx_user_tool_training_expires_at ON user_tool_training(expires_at);

CREATE INDEX idx_tool_trainers_user_id ON tool_trainers(user_id);
CREATE INDEX idx_tool_trainers_tool_id ON tool_trainers(tool_id);