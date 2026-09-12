-- #65 RBAC Phase 1: data model, seeded to reproduce the legacy 5-role ladder.
-- Enforcement still reads users.role in Phase 1; this adds the model + backfill
-- alongside so the resolver can be validated before Phase 2 switches gates onto it.

CREATE TABLE permissions (
    key         TEXT PRIMARY KEY,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE roles (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    -- Protects the seeded legacy roles from deletion/rename by the admin UI.
    is_system   BOOLEAN NOT NULL DEFAULT FALSE,
    -- Preserves the legacy tier ladder for the rank()-style gates (membership,
    -- doors, home-link audiences). A user's effective level = max across roles.
    level       SMALLINT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE role_permissions (
    role_id        UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_key TEXT NOT NULL REFERENCES permissions(key) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_key)
);

-- Role -> role inheritance edges (a role gains the inherited role's permissions,
-- transitively). Self-edges rejected here; deeper cycles guarded in the resolver.
CREATE TABLE role_inheritance (
    role_id          UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    inherits_role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, inherits_role_id),
    CHECK (role_id <> inherits_role_id)
);

CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);

-- Seed: the five legacy roles, levels reproducing the ladder.
INSERT INTO roles (name, description, is_system, level) VALUES
    ('unknown', 'Unknown / unverified (legacy tier 0)', TRUE, 0),
    ('newbie',  'Newbie (legacy tier 1)',               TRUE, 1),
    ('member',  'Member (legacy tier 2)',               TRUE, 2),
    ('staff',   'Staff (legacy tier 3)',                TRUE, 3),
    ('admin',   'Administrator (legacy tier 4)',        TRUE, 4);

-- Coarse access permissions mirroring the legacy can_access_* tiers.
INSERT INTO permissions (key, description) VALUES
    ('member.access', 'Access member-gated routes (legacy can_access_member)'),
    ('staff.access',  'Access staff-gated routes (legacy can_access_staff)'),
    ('admin.access',  'Access admin-gated routes (legacy can_access_admin)');

-- Own grants (inheritance below supplies the rest): member->member.access,
-- staff->staff.access, admin->admin.access.
INSERT INTO role_permissions (role_id, permission_key)
SELECT r.id, k FROM roles r
JOIN (VALUES ('member','member.access'), ('staff','staff.access'), ('admin','admin.access'))
     AS m(rname, k) ON m.rname = r.name;

-- Inheritance chain admin -> staff -> member -> newbie -> unknown, so effective
-- permissions reproduce the ladder: admin has admin+staff+member.access;
-- staff has staff+member.access; member has member.access; newbie/unknown none.
INSERT INTO role_inheritance (role_id, inherits_role_id)
SELECT c.id, p.id FROM roles c
JOIN (VALUES ('admin','staff'), ('staff','member'), ('member','newbie'), ('newbie','unknown'))
     AS e(child, parent) ON e.child = c.name
JOIN roles p ON p.name = e.parent;

-- Backfill: every existing user gets the role matching their current enum value.
INSERT INTO user_roles (user_id, role_id)
SELECT u.id, r.id FROM users u JOIN roles r ON r.name = u.role::text;
