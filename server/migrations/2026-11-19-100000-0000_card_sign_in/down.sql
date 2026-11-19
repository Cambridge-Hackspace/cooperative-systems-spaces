DELETE FROM audit_event_types WHERE name IN ('card_sign_in', 'card_sign_in_second_factor_failed');
DROP TABLE card_sign_ins;
ALTER TABLE resources
    DROP CONSTRAINT resources_sign_in_names_member,
    DROP COLUMN second_factor,
    DROP COLUMN sign_in_app;
