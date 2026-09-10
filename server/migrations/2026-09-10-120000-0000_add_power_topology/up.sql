-- Physical power topology (#41 / #42): circuits -> outlets -> receptacles, plus a
-- tool's optional receptacle. This models the electrical facility so per-circuit
-- draw can later be aggregated (#43) and overloads interrupted (#44).
--
-- A power circuit is NOT placed in the room hierarchy: one circuit can feed
-- outlets in more than one room, so its room membership emerges from its outlets
-- rather than a place_id. Circuits may nest via parent_circuit_id to model an
-- upstream/trunk breaker feeding sub-circuits (self-reference, cycle-guarded at
-- the API layer like places).

CREATE TABLE power_circuits (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    breaker_label     VARCHAR(120) NOT NULL,
    voltage_rating    INTEGER NOT NULL,
    amperage_limit    NUMERIC NOT NULL,
    parent_circuit_id UUID REFERENCES power_circuits(id) ON DELETE RESTRICT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_power_circuits_parent ON power_circuits(parent_circuit_id);

COMMENT ON TABLE  power_circuits                   IS 'A breaker circuit: voltage rating, amperage limit, optional upstream trunk';
COMMENT ON COLUMN power_circuits.parent_circuit_id IS 'Upstream/trunk circuit; NULL for a top-level circuit. RESTRICT prevents orphaning sub-circuits';
COMMENT ON COLUMN power_circuits.amperage_limit    IS 'The circuit rating against which summed draw is checked for overloads (#44)';

-- An outlet necessarily belongs to exactly one room (place), like a door's
-- endpoints; RESTRICT so a room with outlets cannot be deleted out from under
-- them. It draws from exactly one circuit.
CREATE TABLE power_outlets (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    circuit_id UUID NOT NULL REFERENCES power_circuits(id) ON DELETE RESTRICT,
    place_id   UUID NOT NULL REFERENCES places(id) ON DELETE RESTRICT,
    label      VARCHAR(120) NOT NULL,
    location   TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_power_outlets_circuit ON power_outlets(circuit_id);
CREATE INDEX idx_power_outlets_place   ON power_outlets(place_id);

COMMENT ON COLUMN power_outlets.place_id IS 'The room the outlet is in; RESTRICT so a room with outlets cannot be deleted';
COMMENT ON COLUMN power_outlets.location IS 'Free-text position within the room (north wall, bench 3, ...), like doors.location';

-- A receptacle is one physical socket on an outlet.
CREATE TABLE power_receptacles (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    outlet_id  UUID NOT NULL REFERENCES power_outlets(id) ON DELETE CASCADE,
    label      VARCHAR(120) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_power_receptacles_outlet ON power_receptacles(outlet_id);

-- Each tool plugs into zero or one receptacle (battery tools = none). A physical
-- receptacle drives at most one tool, so the assignment is unique where set.
-- Appended last to match the positional Queryable on the Tool model.
ALTER TABLE tools ADD COLUMN receptacle_id UUID REFERENCES power_receptacles(id) ON DELETE SET NULL;
CREATE UNIQUE INDEX uq_tools_receptacle ON tools(receptacle_id) WHERE receptacle_id IS NOT NULL;

COMMENT ON COLUMN tools.receptacle_id IS 'The receptacle this tool plugs into (0 or 1); NULL for battery/unmapped tools. Unique where set';

-- Facility-topology mutation audit, so who changed a circuit's amperage limit
-- (a safety parameter) survives independently of the row.
INSERT INTO audit_event_types (name) VALUES
    ('power_circuit_created'),
    ('power_circuit_updated'),
    ('power_circuit_deleted'),
    ('power_outlet_created'),
    ('power_outlet_updated'),
    ('power_outlet_deleted'),
    ('power_receptacle_created'),
    ('power_receptacle_updated'),
    ('power_receptacle_deleted');
