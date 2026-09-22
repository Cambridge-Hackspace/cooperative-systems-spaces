-- Reverses the schema, but NOT the data. The plaintext codes were deleted by
-- the up migration and cannot be recovered from the sealed columns -- that is
-- the encryption working as intended. Rows return with an empty string; a real
-- rollback restores from a pre-migration database backup.
--
-- Drop the blind-index uniqueness first; the plaintext column and its own
-- unique index are the up migration's concern to restore, and cannot be here
-- anyway -- every non-released row would carry the same empty-string code and
-- collide. A real rollback restores the plaintext (and its index) from backup.
DROP INDEX IF EXISTS idx_user_cards_code_bidx_live;
ALTER TABLE user_cards ADD COLUMN code VARCHAR(255) NOT NULL DEFAULT '';
ALTER TABLE user_cards ALTER COLUMN code DROP DEFAULT;
