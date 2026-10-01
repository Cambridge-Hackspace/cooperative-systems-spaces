-- Reverse of device_bindings 4a: restore tool_modules and move the (tool) rows
-- back. resource_id maps to tool_id (in this slice every binding is a tool's).

CREATE TABLE tool_modules (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id       UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    device_id     UUID NOT NULL REFERENCES space_devices(id) ON DELETE RESTRICT,
    role          TEXT NOT NULL CHECK (role IN ('reader', 'power', 'sensor')),
    name          TEXT NOT NULL,
    params        JSONB NOT NULL DEFAULT '{}'::jsonb,
    on_disconnect TEXT NOT NULL DEFAULT 'fail_off'
                  CHECK (on_disconnect IN ('fail_off', 'hold_last', 'ignore')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_tool_modules_tool   ON tool_modules(tool_id);
CREATE INDEX idx_tool_modules_device ON tool_modules(device_id);

INSERT INTO tool_modules (id, tool_id, device_id, role, name, params, on_disconnect, created_at, updated_at)
    SELECT id, resource_id, device_id, role, name, params, on_disconnect, created_at, updated_at
    FROM device_bindings;

ALTER TABLE tool_interlocks DROP CONSTRAINT tool_interlocks_source_module_id_fkey;
ALTER TABLE tool_interlocks ADD CONSTRAINT tool_interlocks_source_module_id_fkey
    FOREIGN KEY (source_module_id) REFERENCES tool_modules(id) ON DELETE CASCADE;

DROP TABLE device_bindings;
