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

CREATE INDEX profile_config_versions_version_idx ON profile_config_versions (version DESC);
