-- #182: how a session on a resource ends is the resource's to say.
--
-- A door's session has always been implicit -- the pulse -- and a tool's has
-- lasted until a stop. Under one resource model that difference is a profile
-- on the resource, defaulted by kind and changeable: a door whose power
-- bindings stay leased until somebody stops it (a room) is `until_stopped`.
-- The vocabulary is held to `models::resources::session_profile` by
-- checks/tests/text_check_vocabularies_match.rs.
ALTER TABLE resources
    ADD COLUMN session_profile TEXT NOT NULL DEFAULT 'until_stopped'
        CONSTRAINT resources_session_profile_check
        CHECK (session_profile IN ('momentary', 'until_stopped'));

COMMENT ON COLUMN resources.session_profile IS
    'How a session on this resource ends (#182): momentary = over when the strike pulse is; until_stopped = until a stop. Defaulted by kind, changeable by an administrator.';

-- Every door that exists keeps behaving as it did: its session is the pulse.
UPDATE resources SET session_profile = 'momentary' WHERE kind = 'door';

-- The audit event an administrator's change of it writes. An unregistered type
-- is a foreign-key violation that create_audit_log's callers discard.
INSERT INTO audit_event_types (name) VALUES ('resource_updated');
