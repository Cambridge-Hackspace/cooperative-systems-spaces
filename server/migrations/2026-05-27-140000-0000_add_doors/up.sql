-- Door access module: physical doors backed by an edge device, per-door
-- access rule table, persistent unlock log, and a presence (`check-in`)
-- table populated by the QR `I'm here` flow.

CREATE TABLE doors (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(120) NOT NULL,
    location TEXT,
    description TEXT,
    edge_device_id UUID REFERENCES space_devices(id) ON DELETE SET NULL,
    unlock_duration_ms INTEGER NOT NULL DEFAULT 5000,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_doors_edge_device_id ON doors(edge_device_id);

-- Per-door access rules. `deny` always beats `allow` at evaluation time.
-- `value` is interpreted by `kind`:
--   role  -> a user role name (`Member`, `Staff`, `Admin`); means "this role or higher"
--   user  -> a user UUID as text
--   card  -> a literal card identifier
CREATE TABLE door_access_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    door_id UUID NOT NULL REFERENCES doors(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('role','user','card')),
    value TEXT NOT NULL,
    effect TEXT NOT NULL DEFAULT 'allow' CHECK (effect IN ('allow','deny')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (door_id, kind, value, effect)
);
CREATE INDEX idx_door_access_rules_door_id ON door_access_rules(door_id);

-- One row per scan or unlock attempt (granted or denied).
CREATE TABLE door_access_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    door_id UUID NOT NULL REFERENCES doors(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    method TEXT NOT NULL CHECK (method IN ('rfid','qr_checkin','admin_remote')),
    card_id_attempted TEXT,
    granted BOOLEAN NOT NULL,
    reason TEXT,
    ip_address TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_door_access_events_door_id_occurred ON door_access_events(door_id, occurred_at DESC);
CREATE INDEX idx_door_access_events_user_id_occurred ON door_access_events(user_id, occurred_at DESC);

-- Per-checkin presence record, written by the QR `I'm here` flow.
CREATE TABLE door_checkins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    door_id UUID NOT NULL REFERENCES doors(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    door_access_event_id UUID REFERENCES door_access_events(id) ON DELETE SET NULL,
    ip_address TEXT,
    user_agent TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_door_checkins_user_id_occurred ON door_checkins(user_id, occurred_at DESC);

COMMENT ON TABLE doors IS 'Physical doors backed by an edge device';
COMMENT ON TABLE door_access_rules IS 'Per-door access rules; deny beats allow';
COMMENT ON TABLE door_access_events IS 'Every door scan or unlock attempt';
COMMENT ON TABLE door_checkins IS 'Member presence records from the QR `I''m here` flow';

-- Extend the audit_logs CHECK constraint with door events. Webhook
-- subscribers automatically pick these up via the central audit chokepoint.
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check
CHECK (event_type IN (
    -- User
    'user_registration','user_login','user_logout','user_role_change',
    'user_profile_update','user_password_change','user_activation',
    'user_deactivation','user_deletion','admin_config_reload','failed_login_attempt',
    -- Training
    'training_session_started','training_session_completed',
    'training_step_created','training_step_updated','training_step_deleted',
    'trainer_assigned','trainer_removed',
    'instructor_certified','instructor_revoked',
    -- Tool
    'tool_access_granted','tool_access_denied',
    'tool_activated','tool_deactivated','tool_usage_logged',
    -- Device
    'device_invite_created','device_invite_used','device_invite_expired',
    'device_registered','device_name_changed','device_deleted','device_version_changed',
    -- Webhook
    'webhook_created','webhook_updated','webhook_deleted',
    'webhook_auth_header_created','webhook_auth_header_updated','webhook_auth_header_deleted',
    -- MFA
    'mfa_totp_enrolled','mfa_totp_disabled',
    'mfa_webauthn_registered','mfa_webauthn_removed',
    'mfa_recovery_codes_regenerated','mfa_recovery_code_used',
    'mfa_login_passed','mfa_login_failed',
    -- Door
    'door_created','door_updated','door_deleted',
    'door_rule_added','door_rule_removed',
    'door_unlocked_card','door_unlocked_qr','door_unlocked_admin',
    'door_unlock_denied','door_checkin_recorded'
));
