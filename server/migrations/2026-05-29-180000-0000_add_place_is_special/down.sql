DROP INDEX IF EXISTS idx_places_is_special;
ALTER TABLE places DROP COLUMN IF EXISTS is_special;
