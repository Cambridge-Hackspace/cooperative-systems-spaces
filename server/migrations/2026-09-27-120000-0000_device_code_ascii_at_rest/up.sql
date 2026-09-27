-- #120 (#137): device invite codes are emoji, which a non-Unicode database
-- (e.g. a LATIN1 cluster) cannot store, so `POST /api/admin/devices/invite`
-- 500s there and device registration is impossible. Store the code's UTF-8
-- bytes as lowercase ASCII hex instead (server::models::encode_device_code), so
-- invites work on any encoding. Widen the column (hex is longer than the emoji)
-- and re-encode any existing pending invites in place. On a fresh non-Unicode
-- cluster there are no rows to re-encode (emoji could never have been inserted).
ALTER TABLE space_device_auth_requests ALTER COLUMN device_code TYPE TEXT;
UPDATE space_device_auth_requests
SET device_code = encode(convert_to(device_code, 'UTF8'), 'hex')
WHERE device_code !~ '^([0-9a-f][0-9a-f])+$';
