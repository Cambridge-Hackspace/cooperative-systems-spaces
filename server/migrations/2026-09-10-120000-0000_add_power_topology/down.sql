DELETE FROM audit_event_types WHERE name IN (
    'power_circuit_created','power_circuit_updated','power_circuit_deleted',
    'power_outlet_created','power_outlet_updated','power_outlet_deleted',
    'power_receptacle_created','power_receptacle_updated','power_receptacle_deleted'
);

DROP INDEX IF EXISTS uq_tools_receptacle;
ALTER TABLE tools DROP COLUMN IF EXISTS receptacle_id;
DROP TABLE IF EXISTS power_receptacles;
DROP TABLE IF EXISTS power_outlets;
DROP TABLE IF EXISTS power_circuits;
