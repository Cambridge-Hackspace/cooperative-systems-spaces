-- Reversible without loss precisely because `code` was left in place: the
-- plaintext every sealed value was derived from is still the column beside it.
DROP INDEX IF EXISTS idx_user_cards_code_bidx;

ALTER TABLE user_cards
    DROP COLUMN IF EXISTS code_bidx,
    DROP COLUMN IF EXISTS code_nonce,
    DROP COLUMN IF EXISTS code_encrypted;

COMMENT ON COLUMN user_cards.code IS NULL;
