-- Reverses the per-source accumulators (#193).
--
-- `reported_seconds` keeps whatever value it holds, which for an open session
-- with both a plug and a sensor reporting is the sensed figure, not the sum;
-- after this runs, further reports add to it as they did before #193, so such
-- a session settles somewhere between the two rules. Charges already posted
-- are unaffected: they live in membership_ledger keyed on the session.
COMMENT ON COLUMN tool_usage_sessions.reported_seconds IS NULL;
ALTER TABLE tool_usage_sessions
    DROP COLUMN reported_unsensed_seconds,
    DROP COLUMN reported_sensed_seconds;
