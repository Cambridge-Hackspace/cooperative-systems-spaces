DELETE FROM audit_event_types WHERE name IN (
    'circuit_overage_shutoff',
    'emergency_lockout_engaged',
    'emergency_lockout_cleared',
    'firmware_selftrip_reported'
);

ALTER TABLE tool_power_state DROP COLUMN IF EXISTS locked_out_at;
ALTER TABLE tool_power_state DROP COLUMN IF EXISTS lockout_reason;
ALTER TABLE tool_power_state DROP COLUMN IF EXISTS locked_out;

ALTER TABLE power_circuits DROP COLUMN IF EXISTS locked_out_by;
ALTER TABLE power_circuits DROP COLUMN IF EXISTS locked_out_at;
ALTER TABLE power_circuits DROP COLUMN IF EXISTS lockout_source;
ALTER TABLE power_circuits DROP COLUMN IF EXISTS lockout_reason;
ALTER TABLE power_circuits DROP COLUMN IF EXISTS locked_out;
