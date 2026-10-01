-- Device capabilities (#101): a device is no longer a single `kind`.
--
-- One physical unit can fill more than one role -- a reader that also switches
-- power is exactly what ToolPass did -- and the single-valued `space_device_kind`
-- enum could not express it. Replace `kind` with a JSONB `capabilities` blob that
-- carries the roles a device can be bound as (reader / power / sensor) or the
-- coordinator/display roles it fills (edge / kiosk), plus the firmware-enforcement
-- descriptors that used to live on a binding's `params.capabilities`.

ALTER TABLE space_devices
    ADD COLUMN capabilities JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Carry each old single kind across as a one-element roles array. The three tool
-- access modules map to the bindable roles reader / power / sensor; edge and
-- kiosk keep their own names.
UPDATE space_devices SET capabilities = jsonb_build_object(
    'roles',
    jsonb_build_array(
        CASE kind::text
            WHEN 'card_reader' THEN 'reader'
            WHEN 'power_controller' THEN 'power'
            ELSE kind::text
        END
    )
);

-- The role vocabulary, pinned in SQL so `device_capabilities_agree` can compare
-- it to the Rust `device_role` consts. `<@` is jsonb containment: every element
-- of the roles array must appear in this allowed set. A device with no `roles`
-- key (the '{}' default) is permitted here -- requiring a non-empty, in-vocab
-- role set is the registration endpoint's job, so a bad value is a 400 rather
-- than a raw constraint 500.
ALTER TABLE space_devices
    ADD CONSTRAINT space_devices_roles_vocab CHECK (
        capabilities->'roles' IS NULL
        OR (
            jsonb_typeof(capabilities->'roles') = 'array'
            AND capabilities->'roles' <@ '["reader","power","sensor","edge","kiosk"]'::jsonb
        )
    );

DROP INDEX IF EXISTS idx_space_devices_kind;
ALTER TABLE space_devices DROP COLUMN kind;
DROP TYPE space_device_kind;
