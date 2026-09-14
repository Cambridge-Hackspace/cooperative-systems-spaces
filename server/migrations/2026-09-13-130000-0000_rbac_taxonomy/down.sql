-- Reverse #77 Phase 4. Dev rollback: restores the enum column (best-effort
-- primary role from user_roles) and the legacy role names/levels/chain.

CREATE TYPE user_role AS ENUM ('unknown', 'newbie', 'member', 'staff', 'admin');
ALTER TABLE users ADD COLUMN role user_role NOT NULL DEFAULT 'newbie';

-- Restore each user's primary role from their highest-level assignment, mapping
-- the new names back to the legacy enum (active->member, guest/historical->newbie).
UPDATE users u SET role = (
    CASE top.name
        WHEN 'admin' THEN 'admin'
        WHEN 'staff' THEN 'staff'
        WHEN 'active' THEN 'member'
        ELSE 'newbie'
    END
)::user_role
FROM (
    SELECT ur.user_id,
           (SELECT r.name FROM user_roles ur2
              JOIN roles r ON r.id = ur2.role_id
             WHERE ur2.user_id = ur.user_id
             ORDER BY r.level DESC LIMIT 1) AS name
    FROM user_roles ur GROUP BY ur.user_id
) top
WHERE top.user_id = u.id;

-- Restore role names/levels + the unknown role + the legacy chain.
UPDATE roles SET name = 'member', level = 2,
       description = 'Member (legacy tier 2)', updated_at = now()
 WHERE name = 'active';
UPDATE roles SET name = 'newbie', level = 1,
       description = 'Newbie (legacy tier 1)', updated_at = now()
 WHERE name = 'guest';
UPDATE roles SET level = 3, updated_at = now() WHERE name = 'staff';
UPDATE roles SET level = 4, updated_at = now() WHERE name = 'admin';
DELETE FROM roles WHERE name = 'historical';
INSERT INTO roles (name, description, is_system, level)
VALUES ('unknown', 'Unknown / unverified (legacy tier 0)', TRUE, 0);

DELETE FROM role_inheritance;
INSERT INTO role_inheritance (role_id, inherits_role_id)
SELECT c.id, p.id FROM roles c
JOIN (VALUES ('admin', 'staff'), ('staff', 'member'),
             ('member', 'newbie'), ('newbie', 'unknown'))
     AS e(child, parent) ON e.child = c.name
JOIN roles p ON p.name = e.parent;
