-- #120/#9 + #12: two additions to user_mfa_totp, one row per user.
--
-- #9 (pending secret): totp/setup used to DELETE the confirmed row and insert a
-- fresh unconfirmed one, so an abandoned setup -- or an attacker with a hijacked
-- session -- left the account with no usable factor, and the next login 500'd on
-- a user "enrolled" with nothing. The new secret is now held here as *pending*
-- alongside the still-confirmed secret_base32, and promoted only when a code
-- confirms it. NULL means no setup in progress.
ALTER TABLE user_mfa_totp ADD COLUMN pending_secret_base32 TEXT;

-- #12 (replay): a TOTP code was accepted anywhere in its validity window with no
-- record of what had already been spent, so the same six digits replayed within
-- ~30s authenticated twice. The verify path now records the matched time-step
-- here and refuses any code whose step is <= it. NULL means none consumed yet.
ALTER TABLE user_mfa_totp ADD COLUMN last_used_step BIGINT;
