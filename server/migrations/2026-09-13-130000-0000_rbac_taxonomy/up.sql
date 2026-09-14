-- #77 RBAC Phase 4: retire the users.role enum + adopt the
-- guest/historical/active taxonomy. Authorization is fully data-driven via
-- user_roles; the primary-role enum column is removed.
--
-- role ids are stable, so role_permissions (member.access stays owned by the
-- renamed 'active' role) and user_roles assignments follow the renames.

-- 1. Rename the tier roles and renumber levels to guest1/historical2/active3/
--    staff4/admin5.
UPDATE roles SET name = 'active', level = 3,
       description = 'Current member', updated_at = now()
 WHERE name = 'member';
UPDATE roles SET name = 'guest', level = 1,
       description = 'Signed-in, not a member', updated_at = now()
 WHERE name = 'newbie';
UPDATE roles SET level = 4, updated_at = now() WHERE name = 'staff';
UPDATE roles SET level = 5, updated_at = now() WHERE name = 'admin';

-- 2. New role: historical (a former member), the tier between guest and active.
INSERT INTO roles (name, description, is_system, level)
VALUES ('historical', 'Former member, now inactive', TRUE, 2);

-- 3. Retire 'unknown'. A signed-out request has no user (hence no assignment),
--    and the app treats a user with no assignment as a guest, so 'unknown' is
--    redundant. Move any assignments to guest first (dropping duplicates), then
--    delete it (CASCADE clears its inheritance edges + grants).
DELETE FROM user_roles
 WHERE role_id = (SELECT id FROM roles WHERE name = 'unknown')
   AND user_id IN (
       SELECT user_id FROM user_roles
        WHERE role_id = (SELECT id FROM roles WHERE name = 'guest'));
UPDATE user_roles
   SET role_id = (SELECT id FROM roles WHERE name = 'guest')
 WHERE role_id = (SELECT id FROM roles WHERE name = 'unknown');
DELETE FROM roles WHERE name = 'unknown';

-- 4. Rebuild the inheritance chain: admin -> staff -> active -> historical -> guest.
DELETE FROM role_inheritance;
INSERT INTO role_inheritance (role_id, inherits_role_id)
SELECT c.id, p.id FROM roles c
JOIN (VALUES ('admin', 'staff'), ('staff', 'active'),
             ('active', 'historical'), ('historical', 'guest'))
     AS e(child, parent) ON e.child = c.name
JOIN roles p ON p.name = e.parent;

-- 5. Drop the legacy primary-role column + enum type.
ALTER TABLE users DROP COLUMN role;
DROP TYPE user_role;
