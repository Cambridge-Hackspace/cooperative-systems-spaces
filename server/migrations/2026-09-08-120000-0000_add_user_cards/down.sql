DELETE FROM audit_event_types
 WHERE name IN ('revoked_card_presented', 'card_issued', 'card_disabled', 'card_released');

DROP TABLE IF EXISTS user_cards;

DROP TYPE IF EXISTS card_status;
