-- Recreate the retired tables in their final pre-drop shape.
CREATE TABLE tool_trainers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    authorized_by UUID NOT NULL REFERENCES users(id),
    authorized_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    notes TEXT,
    expires_at TIMESTAMP WITH TIME ZONE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, tool_id)
);

CREATE TABLE training_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    trainee_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    trainer_user_id UUID NOT NULL REFERENCES users(id),
    training_date DATE NOT NULL DEFAULT CURRENT_DATE,
    completion_status VARCHAR NOT NULL DEFAULT 'completed',
    minutes_trained INTEGER,
    skills_covered TEXT[],
    notes TEXT,
    next_steps TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    training_step_id UUID REFERENCES training_steps(id) ON DELETE SET NULL
);
