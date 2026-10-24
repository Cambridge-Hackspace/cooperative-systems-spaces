-- Reverses the per-tool usage basis and the second accumulator.
--
-- Any tool set to `active` silently reverts to billing `seconds`, which for a
-- laser is the larger figure -- so a site that had deliberately chosen to bill
-- tube time starts billing bench time at the next settle, with nothing saying
-- so. Check for non-default rows before applying this.
--
-- Accumulated `reported_active_seconds` is dropped and not recoverable. Charges
-- already posted are unaffected: they live in membership_ledger keyed on the
-- session, and settle does not re-run.

ALTER TABLE tool_usage_sessions DROP COLUMN reported_active_seconds;
ALTER TABLE tools DROP COLUMN usage_basis;
