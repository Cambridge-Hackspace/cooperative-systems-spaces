-- The value a device holds for a card (#109).
--
-- Devices authorize offline, so they must hold something to compare a swipe
-- against; today that is the card code itself, in a file on a plug screwed to a
-- wall. This is what replaces it.
--
-- Stored rather than derived per sync because the KDF behind it is deliberately
-- slow: recomputing it for every member on every device's sync would cost tens
-- of seconds of CPU per poll. Stored, it is computed once per card.
--
-- Distinct from code_bidx and NOT interchangeable with it. code_bidx is a fast
-- HMAC whose key never leaves the server; this is a slow KDF whose pepper is
-- distributed to every device. Sending code_bidx to devices instead would mean
-- a stolen plug yields the index key, and index key plus a dump recovers every
-- card in seconds.
--
-- Unfilled here for the same reason as the #108 columns: the key is not
-- reachable from SQL, and must not be. `css-card-backfill` fills it.

ALTER TABLE user_cards ADD COLUMN code_wire_digest BYTEA;

COMMENT ON COLUMN user_cards.code_wire_digest IS
    'argon2id(cards.device_pepper, code). Sent to devices instead of the card value. NOT code_bidx: different key, different speed, different threat.';
