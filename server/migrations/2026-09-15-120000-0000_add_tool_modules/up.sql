-- Tool module bindings + safety interlocks (#83, workstream A: data foundation).
--
-- Formalizes the reader -> power -> sensor -> tool relationship that today is only
-- implicit (tool external_id for readers, the power topology for plugs, and
-- doors.edge_device_id for sensors). A tool_modules row binds one space_device to a
-- tool in a role; tool_interlocks rows are the configurable safety rules.
--
-- Vocabularies are TEXT + CHECK rather than Postgres enums: the permitted set is
-- expected to grow (new sensor conditions), and this mirrors the project's move
-- away from rigid enums (see the audit_event_types lookup table). The permitted
-- values are kept in lockstep with the Rust consts in models/tool_modules.rs by
-- checks/tests/tool_module_vocab_matches.rs.
--
-- Scope note: extending space_device_kind with reader/power/sensor kinds, and the
-- runtime (sync + edge coordinator + lease) live in later increments; this
-- migration is additive (two new tables only), so it is reversible without data
-- loss.

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

COMMENT ON TABLE  tool_modules IS 'Binds a space_device to a tool in a role (reader/power/sensor); the explicit replacement for the implicit external_id / power topology / edge_device_id association (#83)';
COMMENT ON COLUMN tool_modules.role IS 'reader = identity capture; power = governed on/off (references the power topology, does not supersede it -- the outlet supplies, the module governs); sensor = safety input';
COMMENT ON COLUMN tool_modules.params IS 'Role-specific config as JSON: gpio pin, receptacle_id, sensor input, etc.';
COMMENT ON COLUMN tool_modules.on_disconnect IS 'Fail-safe on link loss: fail_off (default; safety-critical power), hold_last, or ignore (#83 sec 6)';

CREATE TABLE tool_interlocks (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tool_id          UUID NOT NULL REFERENCES tools(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('start_gate', 'trip')),
    condition        TEXT NOT NULL CHECK (condition IN (
                        'door_open', 'estop', 'lid_open', 'flow_ok',
                        'authorized', 'auth_expired', 'draw_over', 'module_offline')),
    source_module_id UUID REFERENCES tool_modules(id) ON DELETE CASCADE,
    debounce_ms      INTEGER NOT NULL DEFAULT 0 CHECK (debounce_ms >= 0),
    latch            BOOLEAN NOT NULL DEFAULT TRUE,
    reset            TEXT NOT NULL DEFAULT 're_auth'
                     CHECK (reset IN ('re_auth', 'operator_ack', 'auto')),
    enforcement      TEXT NOT NULL DEFAULT 'edge'
                     CHECK (enforcement IN ('firmware', 'edge', 'server')),
    enabled          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_tool_interlocks_tool   ON tool_interlocks(tool_id);
CREATE INDEX idx_tool_interlocks_source ON tool_interlocks(source_module_id);

COMMENT ON TABLE  tool_interlocks IS 'Configurable safety interlocks for a tool: start gates (AND-ed preconditions) and trips (OR-ed faults) (#83 sec 4)';
COMMENT ON COLUMN tool_interlocks.kind IS 'start_gate = a precondition required to energize; trip = a fault that cuts power';
COMMENT ON COLUMN tool_interlocks.latch IS 'A tripped cut stays off until reset; defaults true (auto-resume on e.g. a laser is unsafe)';
COMMENT ON COLUMN tool_interlocks.enforcement IS 'Where the trip executes: firmware (closest, survives all), edge (cross-module), or server (policy) -- constrained by module capability (#83 sec 5)';
