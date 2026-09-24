-- Drops the pending-secret and replay-step columns. Any setup in progress
-- (a non-null pending_secret_base32) is discarded; confirmed factors are
-- untouched, and replay protection reverts to the pre-#12 behaviour.
ALTER TABLE user_mfa_totp DROP COLUMN last_used_step;
ALTER TABLE user_mfa_totp DROP COLUMN pending_secret_base32;
