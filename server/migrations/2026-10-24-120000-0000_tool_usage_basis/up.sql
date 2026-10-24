-- #169: a device that senses a machine's working signal has two honest answers,
-- and which one bills is ours to decide, not firmware's.
--
-- The driving case is the laser. Its Ruida controller's enable line drops on
-- every scan-line return, so a module watching it can report either the whole
-- engrave as continuous machine time (occupancy) or only the intervals the beam
-- was actually on (active) -- figures that differ by 30-50% on one job. Both are
-- legitimate: occupancy is the bench time nobody else could use, active tracks
-- tube wear. Before this, `tool-log` carried one duration and the choice was
-- made by whatever the firmware author happened to put in it.
--
-- `tools.usage_basis` selects which reported figure the charge is computed from.
-- Default `occupancy` is exactly today's behaviour, where `seconds` is what
-- bills, so no deployed tool changes.
--
-- `tool_usage_sessions.reported_active_seconds` accumulates the other figure
-- regardless of the basis: on an unmetered tool it is pure statistics, and on a
-- metered one it is how an administrator tells a dense engrave from a sparse one
-- at the same bench time. Nullable, because absent means "no device reported a
-- working signal" -- which is not the same as zero, and a tool with no sensing
-- hardware must not look like one that ran for no time at all.
--
-- Appended last on both tables: Tool and ToolUsageSession are positional
-- Queryables.

ALTER TABLE tools
    ADD COLUMN usage_basis TEXT NOT NULL DEFAULT 'occupancy'
        CHECK (usage_basis IN ('occupancy', 'active'));

COMMENT ON COLUMN tools.usage_basis IS
    'Which reported duration the charge uses: occupancy (tool-log `seconds`, the default) or active (tool-log `active_seconds`, the working signal only) (#169).';

ALTER TABLE tool_usage_sessions
    ADD COLUMN reported_active_seconds NUMERIC;

COMMENT ON COLUMN tool_usage_sessions.reported_active_seconds IS
    'Accumulated `active_seconds` from tool-log, recorded whatever the basis. NULL means no device reported a working signal, which is not zero (#169).';
