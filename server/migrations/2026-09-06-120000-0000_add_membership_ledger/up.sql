-- The membership dues ledger: a per-member, non-negative credit account.
--
-- Every payment (Stripe recurring, Stripe one-shot, or an admin-logged cash
-- payment) posts a credit; periodic dues post a debit, but only when the
-- balance can cover them, so the balance never goes negative. Membership
-- entitlement is derived from this ledger; the fast access gate stays
-- users.role. Balance = SUM(amount) per user -- there is no cached balance
-- column, so the ledger is the single source of truth.
--
-- amount is signed: credits are positive, dues/refunds negative. Money is
-- NUMERIC (mapped to BigDecimal), never a float, matching tools.purchase_price.
-- external_reference holds the Stripe invoice/charge id (NOT card data) and is
-- uniquely indexed so a redelivered webhook posts exactly one credit.
--
-- The entry-type enum is deliberately open to extension: Phase 2 (metered
-- pay-per-use tool billing) will add a 'tool_usage' value.
CREATE TYPE ledger_entry_type AS ENUM (
    'stripe_payment',
    'cash_payment',
    'dues_charge',
    'stripe_refund',
    'adjustment'
);

CREATE TABLE membership_ledger (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entry_type ledger_entry_type NOT NULL,
    amount NUMERIC NOT NULL,
    currency TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    description TEXT,
    external_reference TEXT,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_membership_ledger_user_id ON membership_ledger (user_id);

-- Idempotency for external (Stripe) events: at most one ledger row per
-- reference. A partial index so the many manual/dues rows with no reference are
-- not forced unique.
CREATE UNIQUE INDEX idx_membership_ledger_external_reference
    ON membership_ledger (external_reference)
    WHERE external_reference IS NOT NULL;

COMMENT ON TABLE membership_ledger IS
    'Per-member non-negative dues ledger: signed credits (payments) and debits (dues); balance = SUM(amount).';
