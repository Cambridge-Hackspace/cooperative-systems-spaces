DROP INDEX IF EXISTS idx_home_links_expires_at;
ALTER TABLE home_links DROP COLUMN IF EXISTS expires_at;
