-- Per-member billing state for the membership module.
--
-- membership_next_due_at is the enrollment clock: NULL means the user is not
-- enrolled in dues (a never-paid Newbie, or an honorary staff/admin), and the
-- dues logic never touches them. A timestamp is the anniversary at which the
-- next period's dues fall due; the renewal check evaluates it after a grace.
--
-- stripe_customer_id / stripe_subscription_id link the user to Stripe for the
-- Billing Portal and to map inbound webhooks back to exactly one account.
-- subscription_status is the last-seen Stripe status, kept for display only --
-- it is NOT the entitlement gate (the ledger balance is).
--
-- All four are appended with ADD COLUMN (physically last) to match the
-- positional Queryable on the users model, whose new fields must be last.
ALTER TABLE users ADD COLUMN membership_next_due_at TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN stripe_customer_id TEXT;
ALTER TABLE users ADD COLUMN stripe_subscription_id TEXT;
ALTER TABLE users ADD COLUMN subscription_status TEXT;

COMMENT ON COLUMN users.membership_next_due_at IS
    'Enrollment clock: NULL = not enrolled in dues; a timestamp = the anniversary the next dues period falls due.';
COMMENT ON COLUMN users.stripe_customer_id IS
    'Stripe customer id linking this user to the Billing Portal and inbound webhooks. Not card data.';
COMMENT ON COLUMN users.subscription_status IS
    'Last-seen Stripe subscription status, for display only; the ledger balance is the entitlement gate.';
