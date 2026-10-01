-- #118: a user may hold more than one email address.
--
-- Until now `users.email` was the address: one column, two unique constraints
-- (the raw column and lower(email)), and roughly thirty readers that treat it
-- as identity -- login, password reset, verification, the mailing-list sync,
-- Stripe checkout, the TOTP label, the ToolPass loader. The membership roster
-- was loaded from two sources (ToolPass and Stripe) that know the same person
-- by different addresses, and one operator already exists under both the
-- dotted and the dotless form of one Gmail address. Merging those records
-- (the rest of #118) needs somewhere for the second address to go.
--
-- Shape: `user_emails` is the source of truth, one row per address, and
-- `users.email` / `users.email_verified_at` become a MIRROR of the row flagged
-- `is_primary`. The mirror is kept by trigger, in one direction only
-- (user_emails -> users), so:
--
--   * every existing reader of `users.email` keeps working unchanged and keeps
--     meaning "the primary address";
--   * every existing WRITER of a users row -- `create_user`, the ToolPass
--     loader's insert-if-absent, the gap-sync script that inserts by plain SQL
--     -- gets its primary row seeded by the AFTER INSERT trigger without
--     knowing this table exists;
--   * the application can no longer change the primary address by writing
--     `users.email` directly: a BEFORE UPDATE guard refuses a value that is
--     not the current primary row. The one legitimate writer is the mirror
--     trigger itself, which runs after the primary row already says so.
--
-- Uniqueness is global and case-insensitive across every address of every
-- user (the lower(email) index), so a second account can never be registered
-- under a member's secondary address. The partial unique index holds each
-- user to exactly one primary.

CREATE TABLE user_emails (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email VARCHAR NOT NULL,
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    verified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_user_emails_lower ON user_emails (lower(email));
CREATE UNIQUE INDEX idx_user_emails_one_primary ON user_emails (user_id) WHERE is_primary;
CREATE INDEX idx_user_emails_user_id ON user_emails (user_id);

COMMENT ON TABLE user_emails IS
    'Every email address a user holds. The row flagged is_primary is mirrored
     onto users.email / users.email_verified_at by trigger; change the primary
     here, never on users.';
COMMENT ON COLUMN user_emails.verified_at IS
    'When this address was confirmed by its owner. NULL means unconfirmed.';

-- Backfill: every existing account gets its current address as the primary,
-- carrying its verified state over unchanged.
INSERT INTO user_emails (user_id, email, is_primary, verified_at)
SELECT id, email, TRUE, email_verified_at FROM users;

-- user_emails -> users: the primary row is mirrored onto the users columns.
-- Idempotent (only writes when something differs) so the users-side guard
-- below sees a value that the primary row already carries.
CREATE OR REPLACE FUNCTION user_emails_mirror_primary() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.is_primary THEN
        UPDATE users
           SET email = NEW.email,
               email_verified_at = NEW.verified_at
         WHERE id = NEW.user_id
           AND (email IS DISTINCT FROM NEW.email
                OR email_verified_at IS DISTINCT FROM NEW.verified_at);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER user_emails_mirror_primary
    AFTER INSERT OR UPDATE ON user_emails
    FOR EACH ROW EXECUTE FUNCTION user_emails_mirror_primary();

-- users INSERT -> seed the primary row, so every path that creates an account
-- (the API, the loader, a hand-written INSERT) satisfies the invariant
-- without knowing about this table.
CREATE OR REPLACE FUNCTION users_seed_primary_email() RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO user_emails (user_id, email, is_primary, verified_at)
    VALUES (NEW.id, NEW.email, TRUE, NEW.email_verified_at);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_seed_primary_email
    AFTER INSERT ON users
    FOR EACH ROW EXECUTE FUNCTION users_seed_primary_email();

-- users UPDATE guard: the mirrored columns may only take the value the primary
-- row holds. This is what turns "users.email is a mirror" from a convention
-- into an invariant -- a writer that still sets users.email directly fails
-- loudly instead of silently diverging from the table that login reads.
CREATE OR REPLACE FUNCTION users_email_is_a_mirror() RETURNS TRIGGER AS $$
DECLARE
    primary_email TEXT;
    primary_verified TIMESTAMPTZ;
BEGIN
    IF NEW.email IS DISTINCT FROM OLD.email
       OR NEW.email_verified_at IS DISTINCT FROM OLD.email_verified_at THEN
        SELECT email, verified_at
          INTO primary_email, primary_verified
          FROM user_emails
         WHERE user_id = NEW.id AND is_primary;
        IF primary_email IS NULL
           OR primary_email IS DISTINCT FROM NEW.email
           OR primary_verified IS DISTINCT FROM NEW.email_verified_at THEN
            RAISE EXCEPTION
                'users.email and users.email_verified_at mirror the primary user_emails row; change the address there (user %)',
                NEW.id
                USING ERRCODE = 'check_violation';
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_email_is_a_mirror
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION users_email_is_a_mirror();

-- A confirmation token now names the address it confirms. NULL is the
-- primary, which is what every token issued before this column meant.
ALTER TABLE email_verification_tokens
    ADD COLUMN user_email_id UUID REFERENCES user_emails(id) ON DELETE CASCADE;

COMMENT ON COLUMN email_verification_tokens.user_email_id IS
    'The address this token confirms. NULL means the primary address.';

-- Audit vocabulary for the address lifecycle. A primary change keeps emitting
-- the existing user_email_change (the mailing-list sync consumes it).
INSERT INTO audit_event_types (name) VALUES
    ('user_email_added'),
    ('user_email_removed');
