-- Narrowing back to VARCHAR(32) will fail if any stored device_code is longer
-- than 32 bytes -- which is exactly the case this migration exists to allow on a
-- byte-counted cluster. That failure is honest: the down migration cannot
-- restore a limit the data no longer satisfies, and forcing it would truncate a
-- registration code.
ALTER TABLE space_device_auth_requests
    ALTER COLUMN device_code TYPE VARCHAR(32);
