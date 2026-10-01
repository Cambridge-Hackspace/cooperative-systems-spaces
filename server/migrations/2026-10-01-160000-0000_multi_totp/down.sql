-- Reverses #118's multi-TOTP. One row per user can survive: the most recently
-- confirmed one (else the newest). The others are deleted -- a UNIQUE on
-- user_id cannot hold them.
DELETE FROM user_mfa_totp t
 USING (
   SELECT id, row_number() OVER (
            PARTITION BY user_id
            ORDER BY (confirmed_at IS NOT NULL) DESC, confirmed_at DESC NULLS LAST, created_at DESC
          ) AS rn
     FROM user_mfa_totp
 ) ranked
 WHERE t.id = ranked.id AND ranked.rn > 1;

ALTER TABLE user_mfa_totp DROP COLUMN label;

-- pending_secret_base32 sat before last_used_step; positional readers of the
-- pre-#118 model expect that order, so last_used_step is re-appended after it.
ALTER TABLE user_mfa_totp ADD COLUMN pending_secret_base32 TEXT;
ALTER TABLE user_mfa_totp RENAME COLUMN last_used_step TO last_used_step_old;
ALTER TABLE user_mfa_totp ADD COLUMN last_used_step BIGINT;
UPDATE user_mfa_totp SET last_used_step = last_used_step_old;
ALTER TABLE user_mfa_totp DROP COLUMN last_used_step_old;

DROP INDEX IF EXISTS idx_user_mfa_totp_user_id;
ALTER TABLE user_mfa_totp ADD CONSTRAINT user_mfa_totp_user_id_key UNIQUE (user_id);
