-- Decode hex back to the emoji string, then narrow the column. On a non-Unicode
-- cluster the UPDATE re-introduces bytes the column cannot hold and will fail --
-- which is correct: the emoji form is exactly what this migration existed to
-- keep out of such a cluster.
UPDATE space_device_auth_requests
SET device_code = convert_from(decode(device_code, 'hex'), 'UTF8')
WHERE device_code ~ '^([0-9a-f][0-9a-f])+$';
ALTER TABLE space_device_auth_requests ALTER COLUMN device_code TYPE VARCHAR(32);
