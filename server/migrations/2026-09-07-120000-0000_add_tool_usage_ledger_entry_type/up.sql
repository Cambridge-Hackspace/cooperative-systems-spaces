-- Add a 'tool_usage' value to the ledger_entry_type enum for metered
-- pay-per-use tool billing (a debit posted when a tool session settles).
--
-- Done by recreating the type rather than ALTER TYPE ... ADD VALUE: ADD VALUE
-- cannot be used in the same transaction that adds it, and its
-- runs-inside-a-transaction behavior varies by Postgres version. Recreating the
-- type is transaction-safe on every version and is cleanly reversible (the down
-- migration recreates it without the new value). No rows use the new value yet,
-- so the USING cast never fails here.
ALTER TYPE ledger_entry_type RENAME TO ledger_entry_type_old;

CREATE TYPE ledger_entry_type AS ENUM (
    'stripe_payment',
    'cash_payment',
    'dues_charge',
    'stripe_refund',
    'adjustment',
    'tool_usage'
);

ALTER TABLE membership_ledger
    ALTER COLUMN entry_type TYPE ledger_entry_type
    USING entry_type::text::ledger_entry_type;

DROP TYPE ledger_entry_type_old;
