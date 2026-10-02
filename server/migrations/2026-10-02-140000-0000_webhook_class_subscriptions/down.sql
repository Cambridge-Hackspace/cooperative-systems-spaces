-- Reverses #87's class subscriptions. Per-event-type subscriptions are
-- untouched; a webhook that only subscribed by class delivers nothing after.
DROP TABLE IF EXISTS webhook_class_subscriptions;
