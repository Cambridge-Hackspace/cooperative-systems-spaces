-- First-class member access cards (#33): multiple cards per member, each with a
-- lifecycle. Replaces the single-value users.profile[toolguard.profile_field]
-- scheme (which held one card per user and no state). The profile-field path is
-- retained as a compatibility shim in application code, not dropped here.

CREATE TYPE card_status AS ENUM ('active', 'disabled', 'released');

CREATE TABLE user_cards (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code VARCHAR(255) NOT NULL,
    status card_status NOT NULL DEFAULT 'active',
    last_used_at TIMESTAMPTZ,
    issued_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    disabled_at TIMESTAMPTZ,
    released_at TIMESTAMPTZ,
    disabled_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- A "live" code (active or disabled) must resolve to exactly one member, so it is
-- unique among non-released rows. Released codes are returned to the pool and may
-- be reissued to a different member, so they are deliberately excluded from the
-- uniqueness constraint and multiple released rows may share a code.
CREATE UNIQUE INDEX idx_user_cards_code_live
    ON user_cards (code)
    WHERE status <> 'released';

CREATE INDEX idx_user_cards_user ON user_cards (user_id);

-- Distinct, high-signal audit event for presenting a known-but-revoked
-- (disabled/released) card. Denial behaviour is unchanged; this row exists so the
-- event can be hooked for fraud alerting without re-touching the auth path. Added
-- as a lookup row (the idiom since the CHECK constraint was replaced by a table),
-- so a concurrent branch adding its own event types does not collide.
-- Fraud signal (a known-but-revoked card presented) plus the admin card
-- lifecycle actions, so credential changes are auditable.
INSERT INTO audit_event_types (name) VALUES
    ('revoked_card_presented'),
    ('card_issued'),
    ('card_disabled'),
    ('card_released');
