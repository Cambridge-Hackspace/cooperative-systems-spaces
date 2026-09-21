-- Reverses the schema, but NOT the data. The plaintext codes were deleted by
-- the up migration and cannot be recovered from the sealed columns -- that is
-- the encryption working as intended. Rows return with an empty string; a real
-- rollback restores from a pre-migration database backup.
ALTER TABLE user_cards ADD COLUMN code VARCHAR(255) NOT NULL DEFAULT '';
ALTER TABLE user_cards ALTER COLUMN code DROP DEFAULT;
