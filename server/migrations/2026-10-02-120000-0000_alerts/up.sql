-- #87: alerts are audit events with a severity, seen through two permissions,
-- acknowledged beside the row.
--
-- There is no alerts table. An alert is an audit row whose event type the
-- server classifies at Notice or above (the classification lives in Rust,
-- exhaustively, so a new event type cannot arrive unclassified; the database
-- keeps no copy of it). What the database needs is:
--
--   * two permission keys, so who may see and who may acknowledge is a
--     grant an operator edits in the roles screen, never a role name in code;
--   * somewhere to record an acknowledgement that is NOT the audit row --
--     audit_logs is append-only and must stay so;
--   * an audit event type for the acknowledgement itself.

INSERT INTO permissions (key, description) VALUES
    ('alerts.view',        'See the alert feed: audit events classified Notice or above'),
    ('alerts.acknowledge', 'Acknowledge an alert, recording who looked and when');

-- Granted to staff; admin inherits. The same default as the other granular
-- gates; a deployment that wants members to see alerts regrants in the UI.
INSERT INTO role_permissions (role_id, permission_key)
SELECT r.id, k FROM roles r
JOIN (VALUES
        ('staff','alerts.view'),
        ('staff','alerts.acknowledge'))
     AS m(rname, k) ON m.rname = r.name;

CREATE TABLE alert_acknowledgements (
    audit_log_id UUID PRIMARY KEY REFERENCES audit_logs(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    note TEXT,
    acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_alert_acknowledgements_user_id ON alert_acknowledgements (user_id);

COMMENT ON TABLE alert_acknowledgements IS
    'One row per acknowledged alert, keyed by the audit row it acknowledges.
     Beside audit_logs rather than a column on it: the audit stays append-only.';

INSERT INTO audit_event_types (name) VALUES ('alert_acknowledged');
