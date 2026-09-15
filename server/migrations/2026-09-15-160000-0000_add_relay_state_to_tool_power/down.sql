ALTER TABLE tool_power_state
    DROP COLUMN IF EXISTS power_evidence_since,
    DROP COLUMN IF EXISTS last_relay_on;
