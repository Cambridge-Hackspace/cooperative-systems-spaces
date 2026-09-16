-- Relay state and the evidence clock for unauthorized-power detection (#84).
--
-- `last_relay_on` is the first of the two oracles: what the module says its own
-- output is doing. It is NULLABLE because a module that does not report relay
-- state is not the same as one reporting "off", and treating unknown as off
-- would quietly disarm the detector for every plug that cannot answer.
--
-- `power_evidence_since` is when the tool was FIRST seen powered in an
-- uninterrupted run -- relay closed or drawing current. It is the debounce
-- clock, and it is a column rather than server memory so the window survives a
-- restart: a detector that forgot how long a condition had held would either
-- re-arm its debounce (missing a real bypass) or re-report (crying wolf) every
-- time the process bounced.
--
-- Appended last, because the Queryable on ToolPowerState loads positionally.
ALTER TABLE tool_power_state
    ADD COLUMN last_relay_on BOOLEAN,
    ADD COLUMN power_evidence_since TIMESTAMPTZ;

COMMENT ON COLUMN tool_power_state.last_relay_on IS 'Module-reported relay state; NULL when the module does not report it (not the same as off)';
COMMENT ON COLUMN tool_power_state.power_evidence_since IS 'When the tool was first seen powered in an unbroken run (relay closed or drawing); the debounce clock for #84. NULL when not powered';
