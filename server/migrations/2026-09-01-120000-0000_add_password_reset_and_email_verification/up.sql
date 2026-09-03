-- Single-use, expiring tokens for password reset and email verification, and
-- the column that records whether an address was ever confirmed.
--
-- Two tables rather than one with a `purpose` column, deliberately. A shared
-- table means every consume query has to remember `AND purpose = '...'`, and a
-- query that forgets it accepts an email-verification token -- a link sent
-- merely to confirm an address -- as authorization to set a password. That is
-- a privilege escalation whose entire cause is one missing predicate. Two
-- tables are two distinct Diesel types, and the mistake stops being
-- expressible. The duplication is confined to storage: token generation and
-- the atomic claim are shared code.
--
-- Only the SHA-256 of each token is stored. The token itself is in the email
-- and nowhere else, so a database read -- a backup, a stray SELECT, a log
-- spill -- does not hand the reader a working reset link for every pending
-- request. Argon2 is deliberately not used here, unlike user_mfa_recovery_codes:
-- a KDF exists to make a low-entropy secret expensive to guess, and a 256-bit
-- CSPRNG token has no dictionary to slow down. It would also make lookup
-- impossible, since these tokens arrive with no user attached and salted
-- hashes cannot be looked up by value.

CREATE TABLE password_reset_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_password_reset_tokens_user_id ON password_reset_tokens (user_id);
CREATE INDEX idx_password_reset_tokens_expires_at ON password_reset_tokens (expires_at);

COMMENT ON TABLE password_reset_tokens IS
    'Single-use password reset tokens. Claimed atomically; never reusable.';
COMMENT ON COLUMN password_reset_tokens.token_hash IS
    'SHA-256 of the emailed token, hex. The token itself is never stored.';
COMMENT ON COLUMN password_reset_tokens.used_at IS
    'Set by the claiming UPDATE. NULL means live; non-NULL means spent.';

CREATE TABLE email_verification_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_email_verification_tokens_user_id ON email_verification_tokens (user_id);
CREATE INDEX idx_email_verification_tokens_expires_at ON email_verification_tokens (expires_at);

COMMENT ON TABLE email_verification_tokens IS
    'Single-use email confirmation tokens. Deliberately a separate table from
     password_reset_tokens so neither can ever be consumed as the other.';
COMMENT ON COLUMN email_verification_tokens.token_hash IS
    'SHA-256 of the emailed token, hex. The token itself is never stored.';

ALTER TABLE users ADD COLUMN email_verified_at TIMESTAMPTZ;

-- Backfill every existing account as verified.
--
-- This line is the most dangerous one in the change and it is load-bearing.
-- Without it, an operator who sets auth.require_email_verification = true and
-- restarts locks every account that predates this column out of the instance
-- -- their own included, and the bootstrap administrator's -- with no path
-- back in through the API. The recovery would be manual SQL against a live
-- database.
--
-- It is also a line no test can catch the absence of: every test database is
-- created from zero migrations and populated afterwards, so `users` is empty
-- when this runs and the UPDATE is a no-op in exactly the environments where
-- it is exercised. It matters only where it is not tested.
--
-- The flag is meant to apply to accounts created after it was turned on, not
-- retroactively to a membership that had no way to confirm anything.
UPDATE users SET email_verified_at = NOW() WHERE email_verified_at IS NULL;

COMMENT ON COLUMN users.email_verified_at IS
    'When the address was confirmed. NULL means unconfirmed; accounts predating
     the column were backfilled as confirmed.';
