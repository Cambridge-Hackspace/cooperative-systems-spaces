-- Per-member mailing-list intent for the Groups.io integration.
--
-- NULL means subscribed-by-default: a member is on the list once their account
-- is active and their email is verified. A timestamp records an explicit
-- opt-out, set either from the platform toggle or on learning of an unsubscribe
-- performed from a Groups.io email link. Re-subscribing clears it back to NULL.
--
-- Appended (ADD COLUMN puts it physically last) to match the positional
-- Queryable on the users model, whose last field must be the last column.
ALTER TABLE users ADD COLUMN mailing_list_opt_out_at TIMESTAMPTZ;

COMMENT ON COLUMN users.mailing_list_opt_out_at IS
    'When the member opted out of the Groups.io mailing list; NULL means subscribed-by-default once the account is active and the email is verified.';
