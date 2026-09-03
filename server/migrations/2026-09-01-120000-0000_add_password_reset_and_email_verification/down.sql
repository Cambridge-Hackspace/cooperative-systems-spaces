-- Reverse order of application.
ALTER TABLE users DROP COLUMN IF EXISTS email_verified_at;

DROP TABLE IF EXISTS email_verification_tokens;
DROP TABLE IF EXISTS password_reset_tokens;
