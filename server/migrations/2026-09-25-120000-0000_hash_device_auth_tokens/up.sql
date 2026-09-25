-- #120 (#14): device auth tokens were stored in the clear, so any database read
-- (backup, dump, stray SELECT) yielded a working credential for every device.
-- Replace each stored token with its lowercase-hex SHA-256 -- exactly what
-- server::tokens::hash_token produces and what find_device_by_auth_token now
-- looks up. Devices keep their existing token unchanged: the server hashes the
-- value they present and matches on the hash. No re-issuance is required.
--
-- Every pre-existing row holds a plaintext token (a UUID or, after this, a fresh
-- 256-bit token); this hashes whatever is there exactly once. It is a no-op
-- where no devices are registered. convert_to(..., 'UTF8') gives the same bytes
-- Rust hashes (the tokens are ASCII), so the result matches on any DB encoding.
UPDATE space_device_auth
SET auth_token = encode(sha256(convert_to(auth_token, 'UTF8')), 'hex');
