-- #182: the strike is a binding.
--
-- A door's latch used to be described by one column on the door,
-- `doors.unlock_duration_ms`, and driven by whichever device held the door's
-- `edge` binding. That made the latch the one actuator that was not a binding:
-- a tool's relay is a `power` binding with its own profile (#179), a door's
-- strike was a number on the resource. Under one resource model (#101) the
-- latch is a `strike` binding on the device that releases it, and the pulse
-- length and the open-access hold are that binding's profile -- so a room can
-- have a strike AND power bindings, and a door is a resource with a reader and
-- a strike.

-- The role vocabulary. Named constraint, redefined whole, as the coordinator
-- fold did: checks/tests/text_check_vocabularies_match.rs reads the LAST
-- migration naming `device_bindings_role_check` and holds it to
-- `binding_role::ALL`.
ALTER TABLE device_bindings DROP CONSTRAINT device_bindings_role_check;
ALTER TABLE device_bindings ADD CONSTRAINT device_bindings_role_check
    CHECK (role IN ('reader', 'power', 'sensor', 'edge', 'strike'));

-- A device may declare it. Same shape: checks/tests/device_capabilities_agree.rs
-- reads the JSON literal after `<@` in the last migration naming
-- `space_devices_roles_vocab` and holds it to `device_role::ALL`.
ALTER TABLE space_devices DROP CONSTRAINT space_devices_roles_vocab;
ALTER TABLE space_devices ADD CONSTRAINT space_devices_roles_vocab
    CHECK (
        capabilities->'roles' IS NULL
        OR (
            jsonb_typeof(capabilities->'roles') = 'array'
            AND capabilities->'roles' <@ '["reader","power","sensor","edge","kiosk","strike"]'::jsonb
        )
    );

-- The strike profile. Appended last on device_bindings: DeviceBinding is a
-- positional Queryable.
ALTER TABLE device_bindings
    ADD COLUMN pulse_ms INTEGER NOT NULL DEFAULT 0
        CONSTRAINT device_bindings_pulse_nonneg CHECK (pulse_ms >= 0),
    ADD COLUMN hold_open_access BOOLEAN NOT NULL DEFAULT true;

COMMENT ON COLUMN device_bindings.pulse_ms IS
    'strike only (#182): how long the latch is released on a yes, in milliseconds. 0 on every other role.';
COMMENT ON COLUMN device_bindings.hold_open_access IS
    'strike only (#182): whether the latch is held released for the whole of an open-access window. false for a latch that cannot be held energized; the door is then never held.';

-- Every door that could open today keeps opening exactly as it did: its
-- coordinator gets a strike binding carrying the door's own duration. The
-- coordinator is the device the server has always sent the door's unlock to,
-- so it is the device that holds the latch as far as the server knows. A door
-- with no coordinator drove nothing and still drives nothing.
--
-- The binding is created without the device having declared `strike` -- the
-- capability check is the API's, for bindings an administrator authors; this
-- one records what was already true. `ignore` on disconnect: a strike holds
-- nothing energized, so its link dropping is not a reason to cut anything.
INSERT INTO device_bindings (resource_id, device_id, role, name, params, on_disconnect, pulse_ms)
    SELECT b.resource_id, b.device_id, 'strike', d.name || ' strike', '{}'::jsonb, 'ignore',
           d.unlock_duration_ms
    FROM device_bindings b
    JOIN doors d ON d.id = b.resource_id
    WHERE b.role = 'edge'
      AND NOT EXISTS (
          SELECT 1 FROM device_bindings s
          WHERE s.resource_id = b.resource_id AND s.role = 'strike'
      );

-- The column goes. A door with no strike binding uses the site default
-- (`[door] default_unlock_duration_ms`), which is what a new door got here
-- anyway; the value an administrator set lives on the strike binding now.
ALTER TABLE doors DROP COLUMN unlock_duration_ms;
