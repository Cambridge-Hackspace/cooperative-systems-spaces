-- #87: a webhook may ask for a Discord-shaped payload.
--
-- The dispatcher posts the audit event as JSON with an HMAC header: right for
-- a receiver that verifies and parses, and exactly what Discord's incoming
-- webhooks refuse (they accept only {"content": ..} / {"embeds": [..]}). The
-- space already has a Discord channel fed by the backup watchdog; the alert
-- feed should land there without a relay in between. `format` chooses the
-- envelope per webhook: `json` (unchanged default) or `discord`. The events,
-- the subscriptions and the alert feed itself are untouched -- this is only
-- what one delivery looks like on the wire.
--
-- Appended last: Webhook is a positional Queryable.
ALTER TABLE webhooks
    ADD COLUMN format TEXT NOT NULL DEFAULT 'json' CHECK (format IN ('json', 'discord'));

COMMENT ON COLUMN webhooks.format IS
    'Delivery envelope: json (the audit event, signed) or discord (content + embed).';
