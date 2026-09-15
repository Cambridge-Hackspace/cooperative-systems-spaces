-- Recreate the type without the module kinds.
--
-- If any device has actually been registered as a card_reader, power_controller
-- or sensor, the USING cast below fails and the revert aborts. That is the
-- intended behaviour: those rows are real devices, and silently deleting them or
-- rewriting their kind to 'edge' would be worse than refusing. Re-home or delete
-- the module devices first, then revert.
ALTER TYPE space_device_kind RENAME TO space_device_kind_old;

CREATE TYPE space_device_kind AS ENUM (
    'edge',
    'kiosk'
);

ALTER TABLE space_devices
    ALTER COLUMN kind TYPE space_device_kind
    USING kind::text::space_device_kind;

DROP TYPE space_device_kind_old;
