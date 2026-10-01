-- #118: a user may enroll more than one authenticator app.
--
-- `user_mfa_totp.user_id` was UNIQUE: one secret per account. Two members
-- merged into one person can each have enrolled; a member with a phone and a
-- desktop authenticator wants both; and a member replacing a phone wants the
-- new one working before the old one is removed. All three are the same
-- change: drop the one-per-user constraint and give each row a label.
--
-- This also retires `pending_secret_base32`. It existed (#120/#9) so that a
-- setup begun while a confirmed factor was live could not overwrite the
-- working secret -- the only way to express "a second secret" when the table
-- held one row per user. With several rows allowed, a setup in progress is
-- simply an unconfirmed row of its own, and confirming it never touches any
-- other row. The hazard that column guarded is gone with the constraint that
-- created it. An in-flight pending secret at migration time is dropped: it
-- was never confirmed, and the member just runs the setup again.

ALTER TABLE user_mfa_totp DROP CONSTRAINT IF EXISTS user_mfa_totp_user_id_key;
CREATE INDEX IF NOT EXISTS idx_user_mfa_totp_user_id ON user_mfa_totp (user_id);

ALTER TABLE user_mfa_totp DROP COLUMN pending_secret_base32;

-- Appended last: UserMfaTotp is a positional Queryable.
ALTER TABLE user_mfa_totp ADD COLUMN label VARCHAR(120) NOT NULL DEFAULT 'Authenticator app';

COMMENT ON TABLE user_mfa_totp IS
    'A user''s TOTP authenticators (several per user since #118). A row with
     confirmed_at NULL is a setup in progress; login tries every confirmed row.';
COMMENT ON COLUMN user_mfa_totp.label IS 'What the member calls this authenticator.';
