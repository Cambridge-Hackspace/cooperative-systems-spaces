-- Latest-per-tool power telemetry (#43): the most recent reading a tool's
-- controller reported, plus the limits the firmware declares about itself.
--
-- One row per tool (tool_id is the primary key), upserted on every report, so
-- this is the hot-path "current draw" store the per-circuit aggregation (#44)
-- sums over -- NOT a time series. Full history, if wanted, is an optional
-- submodule that mirrors readings to the existing Prometheus wiring; it does
-- not live here.
--
-- The reported max_voltage / amperage_limit are FIRMWARE-owned facts the device
-- reports about itself (this is the honest home for what #35 would have stored
-- as admin config): a per-device over-current limit must fail safe locally, so
-- the device declares it rather than an operator typing it into a form.

CREATE TABLE tool_power_state (
    tool_id                 UUID PRIMARY KEY REFERENCES tools(id) ON DELETE CASCADE,
    last_draw_amps          NUMERIC,
    last_voltage            NUMERIC,
    reported_max_voltage    NUMERIC,
    reported_amperage_limit NUMERIC,
    last_reported_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE  tool_power_state                         IS 'Latest power reading per tool (upserted); the current-draw store #44 aggregates';
COMMENT ON COLUMN tool_power_state.last_draw_amps          IS 'Most recent measured draw (amps) reported by the controller';
COMMENT ON COLUMN tool_power_state.reported_amperage_limit IS 'The over-current limit the firmware declares for itself; firmware-owned, not admin config';
