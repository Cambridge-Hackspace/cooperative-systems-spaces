-- Reverse #101 slice 1a: drop the shared-PK foreign keys, then the supertype.
ALTER TABLE doors DROP CONSTRAINT doors_id_resource_fkey;
ALTER TABLE tools DROP CONSTRAINT tools_id_resource_fkey;
DROP TABLE resources;
