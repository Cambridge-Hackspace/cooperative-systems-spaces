-- #249: a resource can require that every message about it on the local
-- broker is signed.
--
-- Off for everything that exists, and only an administrator turns it on, once
-- the firmware of the devices bound to it signs. Every device registered since
-- #121 holds a key, but firmware that predates #249 sends nothing signed, so
-- requiring it everywhere would refuse every deployed module at once. Any
-- kind: a door's strike is exactly what a forged scan reply opens.
ALTER TABLE resources
    ADD COLUMN require_signed BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN resources.require_signed IS
    'Require signed messages on the local broker (#249): the edge seals what it sends about this resource and refuses anything unsigned. Off unless an administrator turns it on.';
