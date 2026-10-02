-- #87: a webhook may subscribe by CLASS -- a severity floor, optionally within
-- one category -- as well as by individual event type.
--
-- Per-event-type subscriptions (webhook_event_subscriptions) are exact and go
-- stale: "everything critical" written as a list of today's critical types
-- silently misses the next one. A class subscription is evaluated against the
-- event's classification at dispatch time, in Rust, where the classification
-- lives; a new critical event type is delivered the day it exists.
--
-- category NULL means every category. min_severity is one of the Rust
-- Severity vocabulary (checks/tests/alert_severity_vocab_matches.rs holds the
-- CHECK to the enum). Matching: deliver when the event's category equals the
-- subscription's (or the subscription has none) AND the event's severity is at
-- or above min_severity.

CREATE TABLE webhook_class_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    category TEXT,
    min_severity TEXT NOT NULL CHECK (min_severity IN ('info', 'notice', 'warning', 'critical')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_webhook_class_subscriptions_webhook_id ON webhook_class_subscriptions (webhook_id);

COMMENT ON TABLE webhook_class_subscriptions IS
    'Webhook subscriptions by alert class: a severity floor, optionally within
     one category. Matched against the Rust classification at dispatch time.';
