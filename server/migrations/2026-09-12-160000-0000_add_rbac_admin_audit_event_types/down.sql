DELETE FROM audit_event_types WHERE name IN (
    'role_created',
    'role_updated',
    'role_deleted',
    'role_permissions_changed',
    'role_inheritance_changed',
    'user_role_assigned',
    'user_role_unassigned'
);
