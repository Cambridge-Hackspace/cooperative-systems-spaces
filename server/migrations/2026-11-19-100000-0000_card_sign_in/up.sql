-- #241: a card tap at a reader signs the member in to one of the space's own
-- apps (POP, the print tower, first), with an optional second factor.
--
-- sign_in_app: the app a yes on this resource signs in to. Only a resource
-- that already names its member (#228) may carry one: the sign-in is that
-- name, made usable by the app. NULL, as for everything that exists, means
-- a tap signs in to nothing.
--
-- second_factor: who must also give a TOTP code before the sign-in is
-- issued: no one, admins, or everyone.
ALTER TABLE resources
    ADD COLUMN sign_in_app UUID REFERENCES oauth_clients(id) ON DELETE SET NULL,
    ADD COLUMN second_factor TEXT NOT NULL DEFAULT 'off'
        CONSTRAINT resources_second_factor_kind
        CHECK (second_factor IN ('off', 'admins', 'everyone')),
    ADD CONSTRAINT resources_sign_in_names_member
        CHECK (sign_in_app IS NULL OR identify_member);

COMMENT ON COLUMN resources.sign_in_app IS
    'The space''s own app a yes signs the member in to (#241); NULL for none. Needs identify_member.';
COMMENT ON COLUMN resources.second_factor IS
    'Who must also give a TOTP code at a card sign-in (#241): off, admins, everyone.';

-- One card sign-in, from tap to redemption. While a second factor is
-- awaited it has a ticket; once issued, a code. Only their SHA-256 is kept,
-- as for authorization codes. A sign-in is good once and briefly: a ticket
-- for two minutes and three tries, a code for sixty seconds.
CREATE TABLE card_sign_ins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id UUID NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ticket_hash TEXT UNIQUE,
    code_hash TEXT UNIQUE,
    -- How the member proved who they are, for the ID token's amr: 'hwk' (the
    -- card), or 'hwk otp' (the card and a TOTP code).
    amr TEXT NOT NULL,
    tries INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- Set when the code is redeemed. A second redemption is refused, and
    -- what the first issued is revoked.
    used_at TIMESTAMPTZ,
    CONSTRAINT card_sign_ins_ticket_or_code CHECK (ticket_hash IS NOT NULL OR code_hash IS NOT NULL)
);
CREATE INDEX card_sign_ins_user ON card_sign_ins (user_id, created_at);

INSERT INTO audit_event_types (name) VALUES
    ('card_sign_in'),
    ('card_sign_in_second_factor_failed');
