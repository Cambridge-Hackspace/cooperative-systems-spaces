-- Removes the session-revocation epoch. Live tokens issued while it existed
-- carry a token_version claim that nothing checks after this runs -- which is
-- the pre-M9 behaviour (no revocation), not a fault.
ALTER TABLE users DROP COLUMN token_version;
