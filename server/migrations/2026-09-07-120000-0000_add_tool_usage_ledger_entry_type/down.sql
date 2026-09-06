-- Recreate the enum without 'tool_usage'. Assumes the tool-billing feature's
-- rows are gone; the USING cast fails loudly if a tool_usage entry still exists,
-- which is the correct refusal (that history should not be silently dropped).
ALTER TYPE ledger_entry_type RENAME TO ledger_entry_type_old;

CREATE TYPE ledger_entry_type AS ENUM (
    'stripe_payment',
    'cash_payment',
    'dues_charge',
    'stripe_refund',
    'adjustment'
);

ALTER TABLE membership_ledger
    ALTER COLUMN entry_type TYPE ledger_entry_type
    USING entry_type::text::ledger_entry_type;

DROP TYPE ledger_entry_type_old;
