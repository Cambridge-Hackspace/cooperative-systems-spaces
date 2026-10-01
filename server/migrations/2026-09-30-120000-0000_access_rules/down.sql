-- Reverse #101 slice 2b: door_access_rules back from access_rules.
CREATE TABLE door_access_rules (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    door_id     UUID NOT NULL REFERENCES doors(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('role','user','card','open_access')),
    value       TEXT NOT NULL,
    effect      TEXT NOT NULL DEFAULT 'allow' CHECK (effect IN ('allow','deny')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    schedule_id UUID REFERENCES schedules(id) ON DELETE SET NULL,
    UNIQUE (door_id, kind, value, effect)
);
CREATE INDEX idx_door_access_rules_door_id ON door_access_rules(door_id);
INSERT INTO door_access_rules (id, door_id, kind, value, effect, created_at, schedule_id)
SELECT id, resource_id, kind, value, effect, created_at, schedule_id FROM access_rules;
DROP TABLE access_rules;
