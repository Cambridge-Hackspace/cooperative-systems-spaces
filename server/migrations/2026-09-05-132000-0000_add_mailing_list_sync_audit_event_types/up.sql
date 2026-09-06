-- Event types for reconciliation actions and for user email changes.
--
-- mailing_list_sync_add / mailing_list_sync_remove record what reconciliation
-- did to the Groups.io group. user_email_change closes a gap: the account
-- update path changed the address without recording anything, which would let a
-- change silently desync the list.
--
-- Rows, not a restated CHECK: order-independent so a concurrent branch cannot
-- drop one on merge. Dated after the lookup-table migration.
INSERT INTO audit_event_types (name) VALUES
    ('mailing_list_sync_add'),
    ('mailing_list_sync_remove'),
    ('user_email_change');
