-- #120 (#121): a per-device HMAC signing key for the server<->edge command
-- channel, stored SEALED (XChaCha20-Poly1305, like a card code #108) so a
-- database read yields no usable key. NULL for devices registered before this;
-- they run with unsigned commands until re-provisioned (the broker per-device
-- ACLs remain the primary control either way).
ALTER TABLE space_device_auth ADD COLUMN command_key_sealed BYTEA;
ALTER TABLE space_device_auth ADD COLUMN command_key_nonce BYTEA;
