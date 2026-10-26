-- Reverse of up.sql: the duration returns to the door, read back from the
-- door's first strike binding (the site default where it had none), and the
-- strike bindings go with the role.
ALTER TABLE doors ADD COLUMN unlock_duration_ms INTEGER NOT NULL DEFAULT 5000;

UPDATE doors d
SET unlock_duration_ms = s.pulse_ms
FROM (
    SELECT DISTINCT ON (resource_id) resource_id, pulse_ms
    FROM device_bindings
    WHERE role = 'strike' AND pulse_ms > 0
    ORDER BY resource_id, created_at
) s
WHERE s.resource_id = d.id;

DELETE FROM device_bindings WHERE role = 'strike';

ALTER TABLE device_bindings
    DROP COLUMN hold_open_access,
    DROP COLUMN pulse_ms;

ALTER TABLE space_devices DROP CONSTRAINT space_devices_roles_vocab;
ALTER TABLE space_devices ADD CONSTRAINT space_devices_roles_vocab
    CHECK (
        capabilities->'roles' IS NULL
        OR (
            jsonb_typeof(capabilities->'roles') = 'array'
            AND capabilities->'roles' <@ '["reader","power","sensor","edge","kiosk"]'::jsonb
        )
    );

ALTER TABLE device_bindings DROP CONSTRAINT device_bindings_role_check;
ALTER TABLE device_bindings ADD CONSTRAINT device_bindings_role_check
    CHECK (role IN ('reader', 'power', 'sensor', 'edge'));
