-- Reverses #118's merge ledger. The merges themselves are not reversed: the
-- absorbed accounts were deleted and their rows re-pointed, and this table
-- was the only record of that.
DROP TABLE IF EXISTS user_merges;
DELETE FROM audit_event_types WHERE name = 'user_merged';
