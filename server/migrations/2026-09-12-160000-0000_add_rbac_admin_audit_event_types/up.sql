-- #65 RBAC Phase 3c: audit event types for data-driven RBAC administration.
-- Role, matrix, inheritance and user-assignment changes all alter who can do
-- what, so each is auditable. Names match AuditEventType::as_str.
INSERT INTO audit_event_types (name) VALUES
    ('role_created'),
    ('role_updated'),
    ('role_deleted'),
    ('role_permissions_changed'),
    ('role_inheritance_changed'),
    ('user_role_assigned'),
    ('user_role_unassigned');
