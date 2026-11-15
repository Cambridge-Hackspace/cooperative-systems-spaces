-- #226: a registered app messages a member through CS:S, and CS:S delivers it
-- however that member has chosen. The app never learns an address, and its
-- answer is the same whether or not the member gets the message.

-- The kinds of message an app sends, declared by the app, so members can
-- choose per kind. A message of an undeclared kind is refused.
CREATE TABLE notification_kinds (
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    kind TEXT NOT NULL
        CONSTRAINT notification_kinds_kind_shape CHECK (kind ~ '^[a-z][a-z0-9_.-]{0,63}$'),
    label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (client_id, kind)
);

-- A member's choice, per app and kind. `kind = '*'` is the whole app: off
-- means nothing from that app reaches them. With no row, a high-urgency
-- message goes to email and anything else goes nowhere until they opt in.
CREATE TABLE notification_preferences (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    email BOOLEAN NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, client_id, kind)
);

-- Every message accepted, for the dedupe key, the rate limit, and the record.
-- Holds no address and no text: what was sent is the app's to keep.
CREATE TABLE notification_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    urgency TEXT NOT NULL CONSTRAINT notification_messages_urgency CHECK (urgency IN ('normal', 'high')),
    dedupe_key TEXT,
    -- queued, then delivered or failed; or suppressed, when the member chose
    -- not to receive it.
    outcome TEXT NOT NULL
        CONSTRAINT notification_messages_outcome CHECK (outcome IN ('queued', 'delivered', 'failed', 'suppressed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT notification_messages_dedupe UNIQUE (client_id, dedupe_key)
);
CREATE INDEX idx_notification_messages_rate ON notification_messages (client_id, user_id, created_at);
CREATE INDEX idx_notification_messages_user ON notification_messages (user_id);

INSERT INTO audit_event_types (name) VALUES
    ('member_notified'),
    ('notification_kinds_declared');
