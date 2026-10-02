-- Reverses the per-webhook payload format. A Discord-addressed webhook goes
-- back to receiving the raw JSON, which Discord refuses; its deliveries fail
-- until it is deleted.
ALTER TABLE webhooks DROP COLUMN format;
