-- Module device kinds (#83, increment 2): a reader, a power controller and a
-- sensor become devices in their own right, rather than peripherals of the one
-- `edge` node that happens to host them. `edge` stays the local coordinator and
-- `kiosk` is unchanged; the three new kinds are what `tool_modules.device_id`
-- points at when a tool's identity capture, power control and safety sensing are
-- physically separate units.
--
-- Recreating the type rather than ALTER TYPE ... ADD VALUE: ADD VALUE cannot be
-- used in the same transaction that adds it, and its runs-inside-a-transaction
-- behaviour varies by Postgres version. Recreating is transaction-safe on every
-- version and cleanly reversible. No rows use the new values yet, so the USING
-- cast cannot fail here.
ALTER TYPE space_device_kind RENAME TO space_device_kind_old;

CREATE TYPE space_device_kind AS ENUM (
    'edge',
    'kiosk',
    'card_reader',
    'power_controller',
    'sensor'
);

ALTER TABLE space_devices
    ALTER COLUMN kind TYPE space_device_kind
    USING kind::text::space_device_kind;

DROP TYPE space_device_kind_old;

COMMENT ON COLUMN space_devices.kind IS 'edge = local coordinator; kiosk = display; card_reader / power_controller / sensor = tool access modules bound through tool_modules (#83)';
