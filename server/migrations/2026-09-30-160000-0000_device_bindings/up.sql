-- One binding for every access-controlled resource (#101 slice 4a).
--
-- `tool_modules` bound a space_device to a TOOL in a role. A door bound its one
-- coordinator through the ad-hoc `doors.edge_device_id` column instead. Since a
-- door and a tool are both `resources` now (slice 1), the binding is the same
-- relationship keyed on the resource, so it becomes one table. This slice moves
-- the tool bindings; the door coordinator folds in next (4b), which is why the
-- role CHECK is still the tool-role set here.
--
-- resource_id shares tools' ids (a tool's id is its resource id), so the tool
-- bindings carry across with tool_id -> resource_id and no rekey. The binding ids
-- are preserved so tool_interlocks.source_module_id still resolves.

CREATE TABLE device_bindings (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id   UUID NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
    device_id     UUID NOT NULL REFERENCES space_devices(id) ON DELETE RESTRICT,
    role          TEXT NOT NULL CHECK (role IN ('reader', 'power', 'sensor')),
    name          TEXT NOT NULL,
    params        JSONB NOT NULL DEFAULT '{}'::jsonb,
    on_disconnect TEXT NOT NULL DEFAULT 'fail_off'
                  CHECK (on_disconnect IN ('fail_off', 'hold_last', 'ignore')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_device_bindings_resource ON device_bindings(resource_id);
CREATE INDEX idx_device_bindings_device   ON device_bindings(device_id);

COMMENT ON TABLE  device_bindings IS 'Binds a space_device to a resource (a tool or a door) in a role; the one binding replacing tool_modules and doors.edge_device_id (#101)';
COMMENT ON COLUMN device_bindings.role IS 'reader = identity capture; power = governed on/off; sensor = safety input (edge, the door coordinator, is added in slice 4b)';
COMMENT ON COLUMN device_bindings.params IS 'Role-specific config as JSON: gpio pin, receptacle_id, sensor input, etc.';
COMMENT ON COLUMN device_bindings.on_disconnect IS 'Fail-safe on link loss: fail_off (default), hold_last, or ignore (#83 sec 6)';

INSERT INTO device_bindings (id, resource_id, device_id, role, name, params, on_disconnect, created_at, updated_at)
    SELECT id, tool_id, device_id, role, name, params, on_disconnect, created_at, updated_at
    FROM tool_modules;

-- Repoint the interlock source FK from tool_modules to device_bindings. The ids
-- were carried across unchanged, so existing source_module_id values still point
-- at the same binding.
ALTER TABLE tool_interlocks DROP CONSTRAINT tool_interlocks_source_module_id_fkey;
ALTER TABLE tool_interlocks ADD CONSTRAINT tool_interlocks_source_module_id_fkey
    FOREIGN KEY (source_module_id) REFERENCES device_bindings(id) ON DELETE CASCADE;

DROP TABLE tool_modules;
