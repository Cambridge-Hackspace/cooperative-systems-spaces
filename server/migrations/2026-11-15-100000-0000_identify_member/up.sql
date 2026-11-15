-- #228: a resource can name the member who tapped, on a yes.
--
-- Off for everything that exists, and only an administrator turns it on. Only
-- a tool can carry it: an edge decides a door from compiled card digests and
-- holds no principal to name, so a door set to identify would name nobody on
-- the edge's own decisions and somebody on the server's. The constraint keeps
-- that half-working state out of the table, not only out of the API.
ALTER TABLE resources
    ADD COLUMN identify_member BOOLEAN NOT NULL DEFAULT false
        CONSTRAINT resources_identify_member_tool_only
        CHECK (NOT identify_member OR kind = 'tool');

COMMENT ON COLUMN resources.identify_member IS
    'Name the member on a yes (#228): the scan reply carries member { id, name }. Tools only; off unless an administrator turns it on.';
