-- Reverse of door_edge_bindings: put the coordinator back on the doors row.

ALTER TABLE doors ADD COLUMN edge_device_id UUID REFERENCES space_devices(id) ON DELETE SET NULL;

UPDATE doors d SET edge_device_id = b.device_id
    FROM device_bindings b
    WHERE b.resource_id = d.id AND b.role = 'edge';

CREATE INDEX idx_doors_edge_device_id ON doors(edge_device_id);

DROP INDEX IF EXISTS idx_device_bindings_one_edge_per_resource;
DELETE FROM device_bindings WHERE role = 'edge';

ALTER TABLE device_bindings DROP CONSTRAINT device_bindings_role_check;
ALTER TABLE device_bindings ADD CONSTRAINT device_bindings_role_check
    CHECK (role IN ('reader', 'power', 'sensor'));
