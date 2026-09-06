-- Add the 'open_access' door access rule kind.
--
-- An 'open_access' rule holds the door's strike unlocked (no card required)
-- whenever its schedule is active -- the public-access-hours / off-period latch
-- from issue #12. Its `value` column is unused (stored empty); a door expresses
-- multiple public windows as multiple intervals within the one attached
-- schedule, so the existing UNIQUE (door_id, kind, value, effect) is unchanged.
--
-- The kind CHECK on door_access_rules is an inline column constraint, so Postgres
-- auto-named it door_access_rules_kind_check. Drop and recreate it with the new
-- token. ASCII-only.
ALTER TABLE door_access_rules DROP CONSTRAINT IF EXISTS door_access_rules_kind_check;
ALTER TABLE door_access_rules ADD CONSTRAINT door_access_rules_kind_check
    CHECK (kind IN ('role', 'user', 'card', 'open_access'));
