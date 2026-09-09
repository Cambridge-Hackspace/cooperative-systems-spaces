-- Training waivers (#36): a first-class, reason-bearing record that a member's
-- training requirement for a tool is waived.
--
-- This is how legacy ToolPass tool-access grants are represented honestly. Those
-- grants carry no training provenance (no trainer, date, or curriculum), so
-- rather than fabricate a "completed" training step the member never did, we
-- record the truth: access was granted, and here is the mandatory reason
-- ("migrated from ToolPass"). It is also a general primitive for staff
-- discretion, external certification, and grandfathering.
--
-- The mandatory `reason` is the durable provenance: unlike a training-progress
-- note (which cascade-deletes with its step), a waiver's reason is the record
-- itself.

CREATE TABLE training_waivers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    waived_by UUID REFERENCES users(id) ON DELETE SET NULL,
    waived_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- One waiver per (member, tool); re-waiving updates in place.
    UNIQUE (user_id, tool_id)
);

CREATE INDEX idx_training_waivers_user ON training_waivers (user_id);
CREATE INDEX idx_training_waivers_tool ON training_waivers (tool_id);

-- Lifecycle audit, so the fact a waiver was granted/revoked (and by whom)
-- survives independently of the waiver row and its cascade.
INSERT INTO audit_event_types (name) VALUES
    ('training_waiver_granted'),
    ('training_waiver_revoked');
