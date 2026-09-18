-- Card codes encrypted at rest (#108).
--
-- Additive and nullable on purpose. The values cannot be written here: sealing
-- a card needs the deployment's key, and a key reachable from SQL would be a
-- key in the migration file, in the server log, and in `pg_stat_statements` --
-- which is the disclosure this change exists to prevent. The backfill is
-- `css-cli cards backfill`, run once by an operator who has the key.
--
-- `code` is deliberately NOT dropped here. Existing dumps already contain it,
-- so dropping it buys nothing retroactively, and doing it in the same migration
-- would leave no way back if a key turned out to be wrong. The drop is a
-- separate migration after a settling period, once every row is sealed and
-- verified.

ALTER TABLE user_cards
    ADD COLUMN code_encrypted BYTEA,
    ADD COLUMN code_nonce     BYTEA,
    ADD COLUMN code_bidx      BYTEA;

COMMENT ON COLUMN user_cards.code_encrypted IS
    'XChaCha20-Poly1305 sealed card code. Key lives in cards.encryption_key, never here.';
COMMENT ON COLUMN user_cards.code_nonce IS
    'Per-row random nonce. Fresh every seal, which is why code_bidx exists.';
COMMENT ON COLUMN user_cards.code_bidx IS
    'HMAC-SHA256 blind index over the code. Deterministic so resolve_card stays one lookup.';
COMMENT ON COLUMN user_cards.code IS
    'DEPRECATED (#108): plaintext, superseded by code_encrypted. Dropped in a later migration.';

-- Resolution selects on this on every swipe, and a live card is unique per
-- code, so the index carries the same weight the plaintext one did.
CREATE INDEX idx_user_cards_code_bidx ON user_cards (code_bidx);
