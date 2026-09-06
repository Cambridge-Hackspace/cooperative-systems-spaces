-- Per-tool metered-billing rates. A tool is "metered" iff a flat fee or a
-- per-minute rate is set; otherwise it is free (training-gated as before).
--
-- charge = usage_flat_fee + usage_rate_per_min * minutes_used, and
-- usage_max_session_minutes bounds both the prepaid hold estimate and the
-- billable time (NULL falls back to [tool_billing].default_max_session_minutes).
--
-- Appended (ADD COLUMN puts them physically last) to match the positional
-- Queryable on the Tool model, whose new fields must be the last ones.
ALTER TABLE tools ADD COLUMN usage_flat_fee NUMERIC;
ALTER TABLE tools ADD COLUMN usage_rate_per_min NUMERIC;
ALTER TABLE tools ADD COLUMN usage_max_session_minutes INTEGER;

COMMENT ON COLUMN tools.usage_flat_fee IS
    'Optional flat fee charged per use; part of metered billing. NULL/absent = no flat fee.';
COMMENT ON COLUMN tools.usage_rate_per_min IS
    'Optional per-minute usage rate; part of metered billing. NULL/absent = no time charge.';
COMMENT ON COLUMN tools.usage_max_session_minutes IS
    'Caps billable time and the prepaid hold estimate; NULL falls back to the global default.';
