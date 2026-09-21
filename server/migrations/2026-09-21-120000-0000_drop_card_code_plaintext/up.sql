-- #108: remove the last plaintext copy of the card code.
--
-- The sealed columns -- code_encrypted / code_nonce / code_bidx (the
-- 2026-09-18 encrypt migration) and code_wire_digest -- now carry everything a
-- swipe (resolve by blind index) or an edge sync (the device digest) needs.
-- Dropping `code` removes the plaintext that sat in every database backup,
-- which is the whole point of the issue.
--
-- Guarded: refuse to run while any row is still unsealed. Dropping `code` out
-- from under an unsealed row would strand a card that can never be resolved --
-- there is no blind index to find it by, and, once this runs, no plaintext to
-- re-derive one from. The backfill must have reached every row first.
DO $$
DECLARE
    unsealed bigint;
BEGIN
    SELECT count(*) INTO unsealed FROM user_cards WHERE code_bidx IS NULL;
    IF unsealed > 0 THEN
        RAISE EXCEPTION
            'refusing to drop user_cards.code: % row(s) are not sealed yet (code_bidx IS NULL); seal every row before dropping the plaintext',
            unsealed;
    END IF;
END $$;

ALTER TABLE user_cards DROP COLUMN code;
