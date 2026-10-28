-- #193: a plug and a separate sensor on one tool both report `seconds`, and
-- summing them billed the session twice under the occupancy basis.
--
-- A device that senses the machine reports the held working time as `seconds`
-- and sends `active_seconds` beside it; a plug that senses nothing reports the
-- time its power was on and sends no `active_seconds`. Each is the usage that
-- device can honestly measure. Added together they are not a measurement of
-- anything. So the two are kept apart, and `reported_seconds` -- the figure
-- settle bills under `occupancy`, and the one every reader of this table has
-- always read -- is the sensed total when any sensing device has reported on
-- the session, else the unsensed one. The columns are maintained by the same
-- atomic UPDATE that accumulates (database.rs add_reported_seconds); nothing
-- reads them to bill, they are there so the figure can be explained.
ALTER TABLE tool_usage_sessions
    ADD COLUMN reported_sensed_seconds NUMERIC,
    ADD COLUMN reported_unsensed_seconds NUMERIC;

COMMENT ON COLUMN tool_usage_sessions.reported_sensed_seconds IS
    'Accumulated `seconds` from reports that carried `active_seconds`, i.e. from devices that sense the machine (#193). NULL means none has reported.';
COMMENT ON COLUMN tool_usage_sessions.reported_unsensed_seconds IS
    'Accumulated `seconds` from reports that carried no `active_seconds`, i.e. from devices that only switch power (#193). NULL means none has reported.';
COMMENT ON COLUMN tool_usage_sessions.reported_seconds IS
    'The occupancy figure settle bills: reported_sensed_seconds when any sensing device has reported on this session, else reported_unsensed_seconds (#193). Before #193, the sum of every report.';
