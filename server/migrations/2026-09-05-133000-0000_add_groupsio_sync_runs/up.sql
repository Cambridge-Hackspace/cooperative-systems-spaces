-- A log of Groups.io reconciliation runs, for the admin status view.
--
-- One row per pass (ticker or manual). Counts are what the pass changed; ok is
-- whether it completed; error carries the first failure when it did not. Kept
-- as history rather than a single latest-status row so a run that failed is
-- still visible after the next one succeeds.
CREATE TABLE groupsio_sync_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    added INTEGER NOT NULL DEFAULT 0,
    removed INTEGER NOT NULL DEFAULT 0,
    opted_out INTEGER NOT NULL DEFAULT 0,
    ok BOOLEAN NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_groupsio_sync_runs_started_at ON groupsio_sync_runs (started_at DESC);

COMMENT ON TABLE groupsio_sync_runs IS
    'History of Groups.io mailing-list reconciliation passes, for the admin status view.';
