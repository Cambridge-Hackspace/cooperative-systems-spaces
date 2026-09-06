-- A log of membership renewal-cycle runs, for the admin status view.
--
-- One row per pass (daily ticker or a manual "reconcile now"). Counts are what
-- the pass did: users_checked enrolled users evaluated, dues_charged periods
-- deducted, lapsed memberships ended for want of funds, errors individual
-- failures. ok is whether the pass completed; error carries the first failure
-- when it did not. Kept as history rather than one latest-status row so a failed
-- run is still visible after the next one succeeds (mirrors groupsio_sync_runs).
CREATE TABLE membership_sync_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    users_checked INTEGER NOT NULL DEFAULT 0,
    dues_charged INTEGER NOT NULL DEFAULT 0,
    lapsed INTEGER NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    ok BOOLEAN NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_membership_sync_runs_started_at ON membership_sync_runs (started_at DESC);

COMMENT ON TABLE membership_sync_runs IS
    'History of membership renewal-cycle passes, for the admin status view.';
