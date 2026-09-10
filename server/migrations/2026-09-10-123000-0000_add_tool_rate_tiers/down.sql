DELETE FROM audit_event_types WHERE name IN (
    'tool_rate_tier_created',
    'tool_rate_tier_updated',
    'tool_rate_tier_deleted',
    'tool_tier_assigned',
    'tool_tier_unassigned'
);

ALTER TABLE tool_usage_sessions DROP COLUMN IF EXISTS tier_id;
ALTER TABLE tool_usage_sessions DROP COLUMN IF EXISTS rate_per_min;
ALTER TABLE tool_usage_sessions DROP COLUMN IF EXISTS rate_flat_fee;

DROP TABLE IF EXISTS tool_tier_assignments;
DROP TABLE IF EXISTS tool_rate_tiers;
