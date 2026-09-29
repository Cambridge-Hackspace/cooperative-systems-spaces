-- #101 slice 1a: the resources supertype (the spine).
--
-- Doors and tools are the same kind of thing -- an access-controlled resource --
-- but the schema models them as two unrelated tables. This introduces `resources`
-- as the supertype: every door and every tool IS a resource, sharing its id with
-- the subtype row (class-table inheritance on a shared primary key). Sharing the
-- id is what lets the tables keyed on tool_id/door_id keep pointing at the same
-- uuid with no rekey.
--
-- This step is deliberately behaviour-preserving: it adds identity + a `kind`
-- discriminator and the shared-PK foreign keys, and moves no existing column. The
-- common-column lift (name/description/location) is a separate step so each is
-- validated on its own.
CREATE TABLE resources (
    id         uuid PRIMARY KEY,
    kind       text NOT NULL CHECK (kind IN ('door', 'tool')),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Backfill one resource per existing tool and door, sharing the id and preserving
-- the original audit timestamps.
INSERT INTO resources (id, kind, created_at, updated_at)
SELECT id, 'tool', created_at, updated_at FROM tools;

INSERT INTO resources (id, kind, created_at, updated_at)
SELECT id, 'door', created_at, updated_at FROM doors;

-- Every tool/door row IS a resource (shared-PK FK). ON DELETE CASCADE mirrors
-- today's behaviour: deleting the resource removes its subtype.
--
-- DEFERRABLE INITIALLY DEFERRED because a subtype id is generated at insert time:
-- the create paths insert the tool/door row (getting its id) and the parent
-- `resources` row in the same transaction, and the FK is checked at commit rather
-- than per-statement, so insert order within the transaction does not matter.
ALTER TABLE tools
    ADD CONSTRAINT tools_id_resource_fkey
    FOREIGN KEY (id) REFERENCES resources (id) ON DELETE CASCADE
    DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE doors
    ADD CONSTRAINT doors_id_resource_fkey
    FOREIGN KEY (id) REFERENCES resources (id) ON DELETE CASCADE
    DEFERRABLE INITIALLY DEFERRED;
