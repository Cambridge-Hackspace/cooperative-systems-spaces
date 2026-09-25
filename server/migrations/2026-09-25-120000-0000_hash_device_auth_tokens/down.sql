-- Irreversible by construction: a SHA-256 hash cannot be turned back into the
-- token it was computed from. There is nothing to restore -- the plaintext was
-- never anywhere but the device and the original registration response. Reverting
-- this migration therefore leaves the hashed values in place; a device continues
-- to authenticate normally, because the server hashes what it presents either
-- way. (A no-op statement so the revert step has something valid to run.)
SELECT 1;
