-- #87: the heartbeat that makes silence trustworthy.
--
-- A detector that broke looks exactly like a space where nothing is wrong.
-- Once a week the server emits an `alert_heartbeat` audit event -- itself an
-- alert at Notice, so it flows through the same feed and the same webhook
-- subscriptions as everything else -- carrying what happened since the last
-- one: alerts raised, alerts still unacknowledged, webhook deliveries that
-- failed. Its absence, not its content, is the signal.
--
-- Runs are recorded here so the schedule survives restarts: a process that
-- comes up every few days would never reach a weekly tick of its own, so the
-- ticker checks the last recorded run rather than counting from boot.

CREATE TABLE alert_heartbeat_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    alerts_raised INTEGER NOT NULL DEFAULT 0,
    unacknowledged INTEGER NOT NULL DEFAULT 0,
    deliveries_failed INTEGER NOT NULL DEFAULT 0,
    ok BOOLEAN NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_alert_heartbeat_runs_started_at ON alert_heartbeat_runs (started_at DESC);

COMMENT ON TABLE alert_heartbeat_runs IS
    'History of alert heartbeats, newest first. The ticker emits a new one when
     the latest is older than [alerts].heartbeat_interval_secs.';

INSERT INTO audit_event_types (name) VALUES ('alert_heartbeat');
