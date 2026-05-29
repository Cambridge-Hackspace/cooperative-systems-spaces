-- Self-referential, configurable hierarchy of physical places.
-- The level vocabulary (`Facility`, `Building`, `Floor`, `Room`, `Spot`, …)
-- is declared in `[place].types` in config and validated at the API layer.

CREATE TABLE places (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_id   UUID REFERENCES places(id) ON DELETE RESTRICT,
    place_type  TEXT NOT NULL,
    name        VARCHAR(120) NOT NULL,
    description TEXT,
    external_id VARCHAR(255) UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_places_parent_id ON places(parent_id);

-- Siblings under the same parent (and roots together) can't share a name.
CREATE UNIQUE INDEX uq_places_sibling_name
    ON places(COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid), name);

COMMENT ON TABLE  places             IS 'Self-referential tree of physical places';
COMMENT ON COLUMN places.parent_id   IS 'NULL for a root place; RESTRICT prevents orphaning a subtree';
COMMENT ON COLUMN places.place_type  IS 'One of `[place].types` from config (validated at API)';

-- Doors model two sides so we can represent both interior doors (both set)
-- and exterior doors (one NULL = outside / unmodeled).
ALTER TABLE doors          ADD COLUMN place_id_from UUID REFERENCES places(id) ON DELETE SET NULL;
ALTER TABLE doors          ADD COLUMN place_id_to   UUID REFERENCES places(id) ON DELETE SET NULL;
CREATE INDEX idx_doors_place_id_from ON doors(place_id_from);
CREATE INDEX idx_doors_place_id_to   ON doors(place_id_to);

-- Tools and devices each live in exactly one place.
ALTER TABLE tools         ADD COLUMN place_id UUID REFERENCES places(id) ON DELETE SET NULL;
ALTER TABLE space_devices ADD COLUMN place_id UUID REFERENCES places(id) ON DELETE SET NULL;
CREATE INDEX idx_tools_place_id         ON tools(place_id);
CREATE INDEX idx_space_devices_place_id ON space_devices(place_id);

-- Audit events for the new entity. Mirrors the doors / webhooks pattern.
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
    'door_unlock_denied','door_checkin_recorded',
    -- Place
    'place_created','place_updated','place_moved','place_deleted'
));
