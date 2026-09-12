-- #65 RBAC Phase 2: granular permission catalog for the in-handler gates.
--
-- Phase 1 seeded the coarse tier permissions (member/staff/admin.access) that
-- the route extractors resolve. This adds the finer-grained permissions the
-- in-handler authorization checks now consult, and grants them to the `staff`
-- role -- so `admin` holds them too via the admin->staff inheritance seeded in
-- Phase 1, exactly reproducing the old `can_access_staff()` overrides those
-- sites used. Route-level gates stay on the tier permissions for now; splitting
-- them onto domain permissions is a later, incremental step.

INSERT INTO permissions (key, description) VALUES
    ('users.manage',    'Read and modify any user account, including role changes below Admin'),
    ('profiles.manage', 'Read and modify any member profile'),
    ('training.certify','Start and sign off training sessions on behalf of others'),
    ('trainers.manage', 'Manage the set of trainers and their certifications');

-- Granted to staff; admin inherits (Phase 1 seeded admin -> staff). member,
-- newbie and unknown get none, matching the can_access_staff() gates replaced.
INSERT INTO role_permissions (role_id, permission_key)
SELECT r.id, k FROM roles r
JOIN (VALUES
        ('staff','users.manage'),
        ('staff','profiles.manage'),
        ('staff','training.certify'),
        ('staff','trainers.manage'))
     AS m(rname, k) ON m.rname = r.name;
