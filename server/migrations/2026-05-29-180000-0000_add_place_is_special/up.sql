-- Allow operators to mark "special" places (Outside, Common Area, Parking Lot, …).
-- Special places sit outside the normal hierarchy rules: they need not have a
-- parent and their place_type is free-form (not validated against [place].types).
-- Doors/tools/devices reference them like any other place via the existing
-- place_id columns.

ALTER TABLE places
    ADD COLUMN is_special BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX idx_places_is_special ON places(is_special) WHERE is_special;
COMMENT ON COLUMN places.is_special IS
    'When true: place is not part of the normal hierarchy. Free-form place_type and no parent constraint.';
