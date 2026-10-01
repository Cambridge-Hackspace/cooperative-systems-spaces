-- #101 slice 2b: access_rules replaces door_access_rules, keyed on resource_id.
--
-- Doors and tools are both resources; an access rule belongs to a resource, not
-- specifically a door. Since door.id == resource.id (slice 1a), every existing
-- door rule re-points with no id change, and a tool can now carry rules too.
-- Production has 0 rows; the copy is correct regardless.
CREATE TABLE access_rules (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    resource_id UUID NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('role','user','card','open_access')),
    value       TEXT NOT NULL,
    effect      TEXT NOT NULL DEFAULT 'allow' CHECK (effect IN ('allow','deny')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    schedule_id UUID REFERENCES schedules(id) ON DELETE SET NULL,
    UNIQUE (resource_id, kind, value, effect)
);
CREATE INDEX idx_access_rules_resource_id ON access_rules(resource_id);

INSERT INTO access_rules (id, resource_id, kind, value, effect, created_at, schedule_id)
SELECT id, door_id, kind, value, effect, created_at, schedule_id FROM door_access_rules;

DROP TABLE door_access_rules;
COMMENT ON TABLE access_rules IS 'Per-resource access rules (doors + tools); deny beats allow';
