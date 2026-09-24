-- #120/#3 + M8: make e-mail identity case-insensitive at the storage layer.
--
-- Every other part of the system already treats an address case-insensitively:
-- the password-reset and resend paths lowercase before looking up, and the
-- initial-setup admin grant compares case-insensitively. Storage did not:
-- `users.email` was a case-sensitive UNIQUE and the lookup used `email = $1`.
-- Two consequences, both real:
--   * Alice@example.org and alice@example.org registered as two accounts;
--   * a mixed-case registrant could never receive a reset, because the
--     lowercased lookup never matched their stored (mixed-case) address.
-- And, combined with the case-insensitive setup grant, someone could register
-- ADMIN@space.org after the real admin and also be granted admin.
--
-- This adds the missing constraint -- uniqueness over lower(email) -- and the
-- application's lookup switches to `lower(email) = lower($1)`, which this index
-- serves. Stored addresses are left exactly as entered (display is unchanged);
-- only comparison and uniqueness become case-insensitive.
--
-- Guarded: if two rows already differ only by case the unique index cannot be
-- built. Rather than silently pick a winner -- which account survives is an
-- operator's decision, not a migration's -- fail loudly and name them. (Checked
-- clean on chack-css-pub-01 before writing this: 0 colliding groups.)
DO $$
DECLARE
    dupes text;
BEGIN
    SELECT string_agg(le || ' (x' || cnt || ')', ', ')
      INTO dupes
      FROM (
        SELECT lower(email) AS le, count(*) AS cnt
        FROM users
        GROUP BY lower(email)
        HAVING count(*) > 1
      ) d;
    IF dupes IS NOT NULL THEN
        RAISE EXCEPTION
            'refusing to add case-insensitive email uniqueness: these addresses collide only by case and must be reconciled first: %',
            dupes;
    END IF;
END $$;

CREATE UNIQUE INDEX idx_users_email_lower ON users (lower(email));
