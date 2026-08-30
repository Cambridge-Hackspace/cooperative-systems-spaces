-- Append-only version history for the admin-editable user-profile field
-- schema. Rows are never updated or deleted; the current schema is the row
-- with the highest `version`, and a rollback is just inserting a new row
-- with an old row's `profile_fields`.
CREATE TABLE profile_config_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    version BIGINT NOT NULL,
    profile_fields JSONB NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (version)
);

-- Redundant on Postgres specifically: UNIQUE(version) already creates a
-- unique btree index, and Postgres can scan a single-column btree backward
-- just as cheaply as forward, so this adds a little write/space overhead
-- here for no query benefit. Kept for portability -- engines without
-- Postgres's bidirectional scan (e.g. MySQL/InnoDB before 8.0) would
-- actually need this to serve `ORDER BY version DESC` efficiently.
CREATE INDEX profile_config_versions_version_idx ON profile_config_versions (version DESC);
