-- A door's coordinator becomes a device binding (#101 slice 4b-1).
--
-- `doors.edge_device_id` was the last ad-hoc device association left: a tool
-- names its devices through a binding row, a door named exactly one through a
-- column. With doors and tools both resources, the binding serves both, so the
-- column goes and `role = 'edge'` joins the binding vocabulary.
--
-- `edge` was already a declarable DEVICE capability role (slice 3); this is the
-- matching BINDING role, so a device that declares `edge` can now be bound as the
-- coordinator of a door.

ALTER TABLE device_bindings DROP CONSTRAINT device_bindings_role_check;
ALTER TABLE device_bindings ADD CONSTRAINT device_bindings_role_check
    CHECK (role IN ('reader', 'power', 'sensor', 'edge'));

INSERT INTO device_bindings (resource_id, device_id, role, name, on_disconnect)
    SELECT d.id, d.edge_device_id, 'edge', d.name || ' coordinator', 'fail_off'
    FROM doors d
    WHERE d.edge_device_id IS NOT NULL;

-- A resource has at most one coordinator. Without this a second `edge` binding
-- would make "which device drives this door" ambiguous, and the server would
-- pick one arbitrarily -- the kind of thing that presents as a door that unlocks
-- sometimes.
CREATE UNIQUE INDEX idx_device_bindings_one_edge_per_resource
    ON device_bindings(resource_id) WHERE role = 'edge';

DROP INDEX IF EXISTS idx_doors_edge_device_id;
ALTER TABLE doors DROP COLUMN edge_device_id;

COMMENT ON COLUMN device_bindings.role IS 'reader = identity capture; power = governed on/off; sensor = safety input; edge = the local coordinator that drives a door strike (#101)';
