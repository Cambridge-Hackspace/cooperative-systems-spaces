-- Reverses the case-insensitive uniqueness constraint. The case-sensitive
-- UNIQUE on users.email (and the case-insensitive lookup's own tolerance of
-- either casing) remain, so dropping this index cannot orphan a row; it only
-- re-permits two addresses that differ solely by case.
DROP INDEX IF EXISTS idx_users_email_lower;
