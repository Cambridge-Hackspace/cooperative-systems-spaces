-- #118: a user may be more than one Stripe customer.
--
-- `users.stripe_customer_id` was one scalar, and the webhook handlers mapped
-- an inbound event to "the" account through it. Members change the email they
-- gave Stripe, forget they had an account and start another, or arrive from a
-- ToolPass record and a Stripe-only record that #118's merge will fold into
-- one person -- and at that point there are two live customer ids and only one
-- column. A payment on the id that lost would be dropped on the floor (the
-- webhook finds no user and answers 200, which stops Stripe retrying).
--
-- Shape: `user_stripe_customers`, one row per customer id, each carrying the
-- subscription it currently has (if any). The three users columns are DROPPED
-- rather than mirrored: unlike an email there is no single "the" value every
-- reader needs -- checkout and the portal pick the customer with a live
-- subscription (else the most recent), the ledger credits any of them, and
-- "has a subscription" is "any row has one". Only one subscription need be
-- live for a member to be paid up; the ledger keeps all of them.

CREATE TABLE user_stripe_customers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    customer_id TEXT NOT NULL UNIQUE,
    subscription_id TEXT,
    subscription_status TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_user_stripe_customers_user_id ON user_stripe_customers (user_id);

COMMENT ON TABLE user_stripe_customers IS
    'Every Stripe customer id a user is known by. Webhooks map any of them to
     the account; checkout and the Billing Portal use the one with a live
     subscription, else the most recently updated. Never card data.';
COMMENT ON COLUMN user_stripe_customers.subscription_id IS
    'The subscription currently attached to this customer, if any. Cleared on
     customer.subscription.deleted.';
COMMENT ON COLUMN user_stripe_customers.subscription_status IS
    'Last-seen Stripe subscription status, for display. The ledger balance,
     not this, is the entitlement gate.';

INSERT INTO user_stripe_customers (user_id, customer_id, subscription_id, subscription_status)
SELECT id, stripe_customer_id, stripe_subscription_id, subscription_status
  FROM users
 WHERE stripe_customer_id IS NOT NULL;

ALTER TABLE users DROP COLUMN stripe_customer_id;
ALTER TABLE users DROP COLUMN stripe_subscription_id;
ALTER TABLE users DROP COLUMN subscription_status;
