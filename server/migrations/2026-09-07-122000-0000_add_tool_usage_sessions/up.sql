-- One metered tool-use session: the prepaid hold record, the typed usage store,
-- and the idempotency anchor for settling a charge exactly once.
--
-- A session opens when a metered tool is activated (hold_amount reserves the
-- max session cost in prepaid mode; 0 in postpaid), accumulates reported usage,
-- and settles on stop -- posting a single tool_usage ledger debit
-- (ledger_entry_id) of charged_amount and releasing the hold. Available balance
-- for the gate = ledger balance minus the hold_amount of still-open sessions.
CREATE TABLE tool_usage_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    hold_amount NUMERIC NOT NULL,
    reported_seconds NUMERIC,
    charged_amount NUMERIC,
    status TEXT NOT NULL DEFAULT 'open',
    ledger_entry_id UUID REFERENCES membership_ledger(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Available-balance holds sum over each user's still-open sessions.
CREATE INDEX idx_tool_usage_sessions_open_by_user
    ON tool_usage_sessions (user_id)
    WHERE status = 'open';

-- At most one open session per tool (a tool is InUse by one member at a time);
-- also the lookup for correlating a usage report to its session.
CREATE UNIQUE INDEX idx_tool_usage_sessions_one_open_per_tool
    ON tool_usage_sessions (tool_id)
    WHERE status = 'open';

COMMENT ON TABLE tool_usage_sessions IS
    'Metered tool-use sessions: prepaid hold record, usage store, and settle idempotency anchor.';
