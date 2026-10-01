-- Reverses #118's user_stripe_customers. The users columns come back in the
-- order they were added (positional Queryable), carrying each user's customer
-- with a live subscription, else the most recently updated one. Any further
-- customers a user had are lost: the single column cannot hold them.
ALTER TABLE users ADD COLUMN stripe_customer_id TEXT;
ALTER TABLE users ADD COLUMN stripe_subscription_id TEXT;
ALTER TABLE users ADD COLUMN subscription_status TEXT;

UPDATE users u
   SET stripe_customer_id = c.customer_id,
       stripe_subscription_id = c.subscription_id,
       subscription_status = c.subscription_status
  FROM (
    SELECT DISTINCT ON (user_id) user_id, customer_id, subscription_id, subscription_status
      FROM user_stripe_customers
     ORDER BY user_id, (subscription_id IS NOT NULL) DESC, updated_at DESC
  ) c
 WHERE c.user_id = u.id;

-- token_version was the last column before this migration and must stay last
-- for the positional-Queryable reason the User model documents: re-append it.
ALTER TABLE users RENAME COLUMN token_version TO token_version_old;
ALTER TABLE users ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0;
UPDATE users SET token_version = token_version_old;
ALTER TABLE users DROP COLUMN token_version_old;

DROP TABLE IF EXISTS user_stripe_customers;
