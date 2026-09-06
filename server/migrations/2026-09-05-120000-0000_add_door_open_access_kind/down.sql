-- Revert the door access rule kind CHECK to the original three-value set.
-- Any existing 'open_access' rows must be removed first or the new constraint
-- would be violated; deleting them is the correct rollback (the kind no longer
-- exists).
DELETE FROM door_access_rules WHERE kind = 'open_access';
ALTER TABLE door_access_rules DROP CONSTRAINT IF EXISTS door_access_rules_kind_check;
ALTER TABLE door_access_rules ADD CONSTRAINT door_access_rules_kind_check
    CHECK (kind IN ('role', 'user', 'card'));
