-- Emergency lockout state (#44), FAULT-SCOPED:
--   * a CIRCUIT overload (server aggregation, or the edge fast-trip) locks the
--     whole circuit -- no single tool is the culprit;
--   * a FIRMWARE self-trip (one controller over its own over-current limit)
--     locks only THAT tool -- the circuit is fine, the fault is local.
-- A locked circuit or tool is cleared ONLY by a staff member via the site.
--
-- Columns are appended (ADD COLUMN puts them physically last) to match the
-- positional Queryable on the PowerCircuit / ToolPowerState models, whose new
-- fields must be the last ones.

-- Circuit-scoped lockout (aggregation / edge).
ALTER TABLE power_circuits ADD COLUMN locked_out BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE power_circuits ADD COLUMN lockout_reason TEXT;
ALTER TABLE power_circuits ADD COLUMN lockout_source TEXT;
ALTER TABLE power_circuits ADD COLUMN locked_out_at TIMESTAMPTZ;
ALTER TABLE power_circuits ADD COLUMN locked_out_by UUID REFERENCES users(id) ON DELETE SET NULL;

COMMENT ON COLUMN power_circuits.locked_out     IS 'True while the circuit is in emergency lockout; only staff can clear it';
COMMENT ON COLUMN power_circuits.lockout_source IS 'What tripped it: server_aggregate | edge_fast_trip';
COMMENT ON COLUMN power_circuits.locked_out_by  IS 'Staff member who last engaged/cleared it manually; NULL for an automatic trip';

-- Tool-scoped lockout (firmware self-trip).
ALTER TABLE tool_power_state ADD COLUMN locked_out BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE tool_power_state ADD COLUMN lockout_reason TEXT;
ALTER TABLE tool_power_state ADD COLUMN locked_out_at TIMESTAMPTZ;

COMMENT ON COLUMN tool_power_state.locked_out IS 'True while this specific tool is locked out by a firmware self-trip; only staff can clear it';

-- Lifecycle audit, so the fact a circuit tripped (and who cleared it) survives
-- independently of the mutable lockout columns.
INSERT INTO audit_event_types (name) VALUES
    ('circuit_overage_shutoff'),
    ('emergency_lockout_engaged'),
    ('emergency_lockout_cleared'),
    ('firmware_selftrip_reported');
