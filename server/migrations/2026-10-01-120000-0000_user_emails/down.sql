-- Reverses #118's user_emails. The users.email mirror keeps the primary
-- address of every account, so dropping the table loses only secondary
-- addresses -- which have no column to fall back to.
DROP TRIGGER IF EXISTS users_email_is_a_mirror ON users;
DROP FUNCTION IF EXISTS users_email_is_a_mirror();
DROP TRIGGER IF EXISTS users_seed_primary_email ON users;
DROP FUNCTION IF EXISTS users_seed_primary_email();
DROP TRIGGER IF EXISTS user_emails_mirror_primary ON user_emails;
DROP FUNCTION IF EXISTS user_emails_mirror_primary();

ALTER TABLE email_verification_tokens DROP COLUMN IF EXISTS user_email_id;

DROP TABLE IF EXISTS user_emails;

DELETE FROM audit_event_types WHERE name IN ('user_email_added', 'user_email_removed');
