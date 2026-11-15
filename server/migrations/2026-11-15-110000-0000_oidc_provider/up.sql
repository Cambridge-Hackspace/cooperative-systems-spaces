-- #225: CS:S as an OpenID Connect provider.
--
-- Other programs (POP, the print tower, first) sign members in through CS:S
-- with the authorization code flow and PKCE, and hold app-level tokens through
-- the client-credentials grant. Everything secret is stored hashed or sealed:
-- client secrets with argon2, codes and refresh tokens as SHA-256 of 256 random
-- bits, signing keys sealed with ChaCha20-Poly1305.

-- A registered app.
CREATE TABLE oauth_clients (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- What the app sends as client_id. Generated, never chosen.
    client_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    -- argon2 of the secret; NULL for a public client (one that can't keep a
    -- secret, such as a single-page app), which must use PKCE alone.
    secret_hash TEXT,
    confidential BOOLEAN NOT NULL,
    redirect_uris TEXT[] NOT NULL DEFAULT '{}',
    post_logout_redirect_uris TEXT[] NOT NULL DEFAULT '{}',
    allowed_scopes TEXT[] NOT NULL DEFAULT '{openid}',
    -- The space's own programs may skip the consent screen; third-party ones
    -- never should.
    skip_consent BOOLEAN NOT NULL DEFAULT false,
    disabled BOOLEAN NOT NULL DEFAULT false,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT oauth_clients_secret_matches_type
        CHECK ((confidential AND secret_hash IS NOT NULL) OR (NOT confidential AND secret_hash IS NULL))
);

-- The keys ID tokens and access tokens are signed with. The newest unretired
-- one signs; a retired one stays published until its tokens have expired.
CREATE TABLE oauth_signing_keys (
    kid TEXT PRIMARY KEY,
    private_key_sealed TEXT NOT NULL,
    public_jwk JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    retired_at TIMESTAMPTZ
);

-- An authorization request waiting for the member: sign-in, consent, or both.
-- The browser arrives cross-site, so CS:S's SameSite=Strict session isn't sent
-- on that first request; the request is parked here and finished by CS:S's
-- own page, where the session is.
CREATE TABLE oauth_authorization_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    scope TEXT NOT NULL,
    state TEXT,
    nonce TEXT,
    code_challenge TEXT NOT NULL,
    prompt TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE oauth_authorization_codes (
    code_hash TEXT PRIMARY KEY,
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    scope TEXT NOT NULL,
    nonce TEXT,
    code_challenge TEXT NOT NULL,
    auth_time TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- Set on first use. A second use is refused, and every refresh token the
    -- first use issued is revoked (RFC 6749 section 4.1.2).
    used_at TIMESTAMPTZ
);

CREATE TABLE oauth_refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash TEXT NOT NULL UNIQUE,
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    auth_time TIMESTAMPTZ NOT NULL,
    -- The code this chain started from, so a replayed code ends the chain.
    code_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX idx_oauth_refresh_tokens_user_client ON oauth_refresh_tokens (user_id, client_id);
CREATE INDEX idx_oauth_refresh_tokens_code_hash ON oauth_refresh_tokens (code_hash);

-- What a member has agreed to share with an app, so they aren't asked again.
CREATE TABLE oauth_consents (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    scopes TEXT[] NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, client_id)
);

-- Managing apps is a grant in the roles screen, not a role name in code.
INSERT INTO permissions (key, description) VALUES
    ('oauth.clients.manage', 'Register the apps that may sign members in through CS:S, and change or remove them');
INSERT INTO role_permissions (role_id, permission_key)
SELECT r.id, 'oauth.clients.manage' FROM roles r WHERE r.name = 'admin';

INSERT INTO audit_event_types (name) VALUES
    ('oauth_client_created'),
    ('oauth_client_updated'),
    ('oauth_client_secret_rotated'),
    ('oauth_client_deleted'),
    ('oauth_consent_granted'),
    ('oauth_signin'),
    ('oauth_signing_key_rotated');
