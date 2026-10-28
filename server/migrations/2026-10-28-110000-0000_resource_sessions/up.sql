-- #190: a session is a fact about a resource, not a column on a tool.
--
-- A tool's session has always been `tools.status = in_use` plus the activation
-- event; a door's was nobody's -- the coordinator held it, the server kept no
-- record, and so the platform could not show a room in use, name who opened
-- it, let an administrator end it, or sweep one abandoned with its power on,
-- and a restarted coordinator started from nothing. This table is the record,
-- written by the one scan path for an `until_stopped` door on a yes and closed
-- by the stop, the sweep or an administrator. Tools keep `tools.status` for
-- now; the module-state payload and the admin view read both and present one
-- fact per resource, which is the half of "one sessions concept" this change
-- delivers. At most one open session per resource, by the partial index.
CREATE TABLE resource_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id UUID NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    device_id UUID REFERENCES space_devices(id) ON DELETE SET NULL,
    opened_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ,
    closed_by TEXT
        CONSTRAINT resource_sessions_closed_by_check
        CHECK (closed_by IS NULL OR closed_by IN ('stop', 'sweep', 'admin'))
);

CREATE UNIQUE INDEX resource_sessions_one_open
    ON resource_sessions (resource_id) WHERE closed_at IS NULL;
CREATE INDEX resource_sessions_by_resource
    ON resource_sessions (resource_id, opened_at DESC);

COMMENT ON TABLE resource_sessions IS
    'An access session on a resource of any kind (#190): opened by a badge on an until_stopped resource, closed by a stop, the sweep or an administrator. One open per resource.';
COMMENT ON COLUMN resource_sessions.closed_by IS
    'Who ended it: stop (a device), sweep (the session sweep), admin (an administrator). NULL while open. Vocabulary held to models::resources::session_closed_by.';

-- The audit events the two new ways of ending one write. An unregistered type
-- is a foreign-key violation that create_audit_log's callers discard.
INSERT INTO audit_event_types (name) VALUES
    ('resource_session_ended'),
    ('resource_session_released_by_sweep');
