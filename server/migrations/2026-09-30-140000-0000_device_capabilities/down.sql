-- Reverse of device_capabilities: restore the single-valued space_device_kind
-- enum and drop the capabilities blob.
CREATE TYPE space_device_kind AS ENUM (
    'edge',
    'kiosk',
    'card_reader',
    'power_controller',
    'sensor'
);

ALTER TABLE space_devices ADD COLUMN kind space_device_kind;

-- Best-effort inverse: take the first declared role and map the bindable roles
-- back to their module-kind names. A device that declared several roles cannot be
-- represented by the single enum and keeps only its first; anything unrecognised
-- (or a device with no roles) falls back to edge so the NOT NULL can be set.
UPDATE space_devices SET kind = (
    CASE capabilities->'roles'->>0
        WHEN 'reader' THEN 'card_reader'
        WHEN 'power' THEN 'power_controller'
        WHEN 'sensor' THEN 'sensor'
        WHEN 'kiosk' THEN 'kiosk'
        ELSE 'edge'
    END
)::space_device_kind;

ALTER TABLE space_devices ALTER COLUMN kind SET NOT NULL;
CREATE INDEX idx_space_devices_kind ON space_devices(kind);

ALTER TABLE space_devices DROP CONSTRAINT space_devices_roles_vocab;
ALTER TABLE space_devices DROP COLUMN capabilities;
