-- #118: the record of one user being merged into another.
--
-- A merge re-points every row that referenced the absorbed account to the
-- survivor and then deletes the absorbed row, so afterwards nothing in the
-- schema says the second account ever existed. This table does. It keeps the
-- absorbed account's own fields (never its password hash), what was moved,
-- and every warning the administrator acknowledged before committing, so a
-- merge made in error can be reconstructed by hand and a dispute can be
-- answered. absorbed_id carries no foreign key: the row it names is gone,
-- which is the point.

CREATE TABLE user_merges (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    survivor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    absorbed_id UUID NOT NULL,
    absorbed_username TEXT NOT NULL,
    absorbed_email TEXT NOT NULL,
    absorbed_snapshot JSONB NOT NULL,
    moved JSONB NOT NULL,
    warnings JSONB NOT NULL,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_user_merges_survivor_id ON user_merges (survivor_id);
CREATE INDEX idx_user_merges_absorbed_id ON user_merges (absorbed_id);

COMMENT ON TABLE user_merges IS
    'One row per user merge: the absorbed account''s fields (no password hash),
     the rows moved per table, and the warnings the administrator acknowledged.';

INSERT INTO audit_event_types (name) VALUES ('user_merged');
