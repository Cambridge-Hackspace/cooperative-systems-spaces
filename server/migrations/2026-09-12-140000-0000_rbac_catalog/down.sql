DELETE FROM role_permissions WHERE permission_key IN
    ('users.manage', 'profiles.manage', 'training.certify', 'trainers.manage');
DELETE FROM permissions WHERE key IN
    ('users.manage', 'profiles.manage', 'training.certify', 'trainers.manage');
