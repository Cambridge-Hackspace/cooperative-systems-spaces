DELETE FROM audit_event_types WHERE name IN (
    'oauth_client_created', 'oauth_client_updated', 'oauth_client_secret_rotated',
    'oauth_client_deleted', 'oauth_consent_granted', 'oauth_signin', 'oauth_signing_key_rotated');
DELETE FROM role_permissions WHERE permission_key = 'oauth.clients.manage';
DELETE FROM permissions WHERE key = 'oauth.clients.manage';
DROP TABLE oauth_consents;
DROP TABLE oauth_refresh_tokens;
DROP TABLE oauth_authorization_codes;
DROP TABLE oauth_authorization_requests;
DROP TABLE oauth_signing_keys;
DROP TABLE oauth_clients;
