-- Reverses #179's per-binding sequencing timings.
ALTER TABLE device_bindings
    DROP COLUMN IF EXISTS run_on_ms,
    DROP COLUMN IF EXISTS start_delay_ms,
    DROP COLUMN IF EXISTS start_stage;
