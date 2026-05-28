-- Webhook system: admin-defined outbound webhooks fired on audit events,
-- with reusable write-only auth credentials and per-webhook HMAC signing.

-- Reusable, write-only auth credentials (e.g. an Authorization header).
-- header_value is never returned by the API.
CREATE TABLE webhook_auth_headers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    header_name VARCHAR(255) NOT NULL,
    header_value TEXT NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Webhook definitions.
CREATE TABLE webhooks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    url TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    signing_secret TEXT NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Which audit event types each webhook subscribes to.
CREATE TABLE webhook_event_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (webhook_id, event_type)
);

-- Which reusable auth headers attach to each webhook.
CREATE TABLE webhook_auth_links (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    auth_header_id UUID NOT NULL REFERENCES webhook_auth_headers(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (webhook_id, auth_header_id)
);

-- Delivery log: one row per attempt.
CREATE TABLE webhook_deliveries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_id UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    audit_log_id UUID REFERENCES audit_logs(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 1,
    success BOOLEAN NOT NULL DEFAULT FALSE,
    status_code INTEGER,
    response_body TEXT,
    error TEXT,
    request_payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_webhook_event_subscriptions_event_type ON webhook_event_subscriptions(event_type);
CREATE INDEX idx_webhook_event_subscriptions_webhook_id ON webhook_event_subscriptions(webhook_id);
CREATE INDEX idx_webhook_auth_links_webhook_id ON webhook_auth_links(webhook_id);
CREATE INDEX idx_webhook_auth_links_auth_header_id ON webhook_auth_links(auth_header_id);
CREATE INDEX idx_webhook_deliveries_webhook_id ON webhook_deliveries(webhook_id);
CREATE INDEX idx_webhook_deliveries_created_at ON webhook_deliveries(created_at DESC);

COMMENT ON TABLE webhook_auth_headers IS 'Reusable, write-only auth credentials attached to webhooks';
COMMENT ON COLUMN webhook_auth_headers.header_value IS 'Secret value; never returned via the API';
COMMENT ON TABLE webhooks IS 'Admin-defined outbound webhooks fired on audit events';
COMMENT ON COLUMN webhooks.signing_secret IS 'Per-webhook secret for HMAC-SHA256 request signing';
COMMENT ON TABLE webhook_event_subscriptions IS 'Audit event types each webhook subscribes to';
COMMENT ON TABLE webhook_auth_links IS 'Reusable auth headers attached to each webhook';
COMMENT ON TABLE webhook_deliveries IS 'Per-attempt log of webhook delivery results';

-- Add webhook management audit events to the audit_logs CHECK constraint.
ALTER TABLE audit_logs DROP CONSTRAINT IF EXISTS audit_logs_event_type_check;

ALTER TABLE audit_logs ADD CONSTRAINT audit_logs_event_type_check
CHECK (event_type IN (
    -- User-related events
    'user_registration',
    'user_login',
    'user_logout',
    'user_role_change',
    'user_profile_update',
    'user_password_change',
    'user_activation',
    'user_deactivation',
    'user_deletion',
    'admin_config_reload',
    'failed_login_attempt',
    -- Training-related events
    'training_session_started',
    'training_session_completed',
    'training_step_created',
    'training_step_updated',
    'training_step_deleted',
    'trainer_assigned',
    'trainer_removed',
    'instructor_certified',
    'instructor_revoked',
    -- Tool-related events
    'tool_access_granted',
    'tool_access_denied',
    'tool_activated',
    'tool_deactivated',
    'tool_usage_logged',
    -- Device-related events
    'device_invite_created',
    'device_invite_used',
    'device_invite_expired',
    'device_registered',
    'device_name_changed',
    'device_deleted',
    'device_version_changed',
    -- Webhook management events
    'webhook_created',
    'webhook_updated',
    'webhook_deleted',
    'webhook_auth_header_created',
    'webhook_auth_header_updated',
    'webhook_auth_header_deleted'
));
