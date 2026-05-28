-- Multi-factor authentication: per-user TOTP secret, WebAuthn credentials,
-- and one-time recovery codes. Each table cascades on user delete.

-- Quick "does this user have any MFA" flag, set when the first method is
-- confirmed and cleared when the last is removed.
ALTER TABLE users ADD COLUMN mfa_enrolled_at TIMESTAMPTZ;

-- TOTP (authenticator app). At most one per user. `confirmed_at` is set on
-- successful first verification; unconfirmed rows are setup-in-progress.
CREATE TABLE user_mfa_totp (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    secret_base32 TEXT NOT NULL,
    confirmed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- WebAuthn / FIDO2 credentials (Yubikey, Touch ID, Windows Hello, passkeys).
-- `passkey` stores the serialized `webauthn_rs::prelude::Passkey` so the
-- library can verify authentication assertions on its own.
CREATE TABLE user_mfa_webauthn (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credential_id BYTEA NOT NULL UNIQUE,
    passkey JSONB NOT NULL,
    label VARCHAR(120) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX idx_user_mfa_webauthn_user_id ON user_mfa_webauthn(user_id);

-- One-time recovery codes. Stored as Argon2 hashes so a DB read can't yield
-- usable codes. `used_at` is set when consumed; rows are kept for audit.
CREATE TABLE user_mfa_recovery_codes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_user_mfa_recovery_user_id ON user_mfa_recovery_codes(user_id);

COMMENT ON TABLE user_mfa_totp IS 'Per-user TOTP secret (authenticator app)';
COMMENT ON TABLE user_mfa_webauthn IS 'Per-user WebAuthn credentials (Yubikey/Touch ID/passkey)';
COMMENT ON TABLE user_mfa_recovery_codes IS 'One-time MFA recovery codes (Argon2-hashed)';

-- Extend the audit_logs CHECK constraint with MFA events.
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check
CHECK (event_type IN (
    -- User-related
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
    -- Webhook management
    'webhook_created','webhook_updated','webhook_deleted',
    'webhook_auth_header_created','webhook_auth_header_updated','webhook_auth_header_deleted',
    -- MFA
    'mfa_totp_enrolled','mfa_totp_disabled',
    'mfa_webauthn_registered','mfa_webauthn_removed',
    'mfa_recovery_codes_regenerated','mfa_recovery_code_used',
    'mfa_login_passed','mfa_login_failed'
));
