-- Optional expiry timestamp for home links. Used for announcement-style links
-- ("Open House this Friday") that should self-hide after the event passes.
-- NULL = never expires (default behavior preserved for existing rows).

ALTER TABLE home_links
    ADD COLUMN expires_at TIMESTAMPTZ;

CREATE INDEX idx_home_links_expires_at ON home_links(expires_at)
    WHERE expires_at IS NOT NULL;

COMMENT ON COLUMN home_links.expires_at IS
    'When set: link is hidden from the public endpoint once now() >= expires_at.';
