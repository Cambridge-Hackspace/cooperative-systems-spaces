-- Per-member tool rate tiers (#34). ToolPass assigns a named rate tier per member
-- per tool (Standard / Discounted / Free / ...); CSS today charges a single
-- per-tool rate. This adds named tiers per tool and a per-(member,tool)
-- assignment. The tool''s own usage_* columns stay as the DEFAULT ("Standard")
-- rate, so no data migration is needed and an unassigned member bills at the
-- default.

CREATE TABLE tool_rate_tiers (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id             UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    name                VARCHAR(120) NOT NULL,
    flat_fee            NUMERIC,
    rate_per_min        NUMERIC,
    max_session_minutes INTEGER,
    active              BOOLEAN NOT NULL DEFAULT true,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tool_id, name)
);
CREATE INDEX idx_tool_rate_tiers_tool ON tool_rate_tiers(tool_id);

COMMENT ON TABLE  tool_rate_tiers              IS 'Named per-tool rate tiers (#34); the tool''s own usage_* columns are the default/Standard tier';
COMMENT ON COLUMN tool_rate_tiers.rate_per_min IS 'Per-minute rate for this tier; NULL behaves like the tool columns (no time charge). A Free tier is rate 0.';

CREATE TABLE tool_tier_assignments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tool_id     UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    tier_id     UUID NOT NULL REFERENCES tool_rate_tiers(id) ON DELETE CASCADE,
    assigned_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, tool_id)
);
CREATE INDEX idx_tool_tier_assignments_user ON tool_tier_assignments(user_id);
CREATE INDEX idx_tool_tier_assignments_tool ON tool_tier_assignments(tool_id);
CREATE INDEX idx_tool_tier_assignments_tier ON tool_tier_assignments(tier_id);

COMMENT ON TABLE tool_tier_assignments IS 'A member''s rate tier for a tool (#34); absent = the tool default. One per (member,tool).';

-- Lock the resolved rate onto the session at tool-on, for a stable charge and
-- provenance. Appended (ADD COLUMN puts them physically last) to match the
-- positional Queryable on the ToolUsageSession model.
ALTER TABLE tool_usage_sessions ADD COLUMN rate_flat_fee NUMERIC;
ALTER TABLE tool_usage_sessions ADD COLUMN rate_per_min  NUMERIC;
ALTER TABLE tool_usage_sessions ADD COLUMN tier_id       UUID REFERENCES tool_rate_tiers(id) ON DELETE SET NULL;

COMMENT ON COLUMN tool_usage_sessions.rate_per_min IS 'Resolved per-minute rate captured at tool-on (member''s tier, else tool default); the settle charge uses this, not the live tool rate';
COMMENT ON COLUMN tool_usage_sessions.tier_id      IS 'The tier applied to this session; NULL = the tool default rate was used';

INSERT INTO audit_event_types (name) VALUES
    ('tool_rate_tier_created'),
    ('tool_rate_tier_updated'),
    ('tool_rate_tier_deleted'),
    ('tool_tier_assigned'),
    ('tool_tier_unassigned');
