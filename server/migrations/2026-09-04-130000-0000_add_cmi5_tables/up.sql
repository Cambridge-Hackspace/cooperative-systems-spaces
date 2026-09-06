-- cmi5 (xAPI) training-module support: course structure, launch sessions, the
-- embedded-minimal LRS store, and the State API document store.
--
-- Enum-like columns (move_on, launch_method, launch_mode) are TEXT rather than
-- Postgres enums. Their only writer is the import path, which sources them from
-- a manifest the `cmi5` crate has already parsed and validated, so the integrity
-- a CHECK/enum would add is already enforced one layer up -- and a TEXT column
-- avoids adding a new sql_type and the CHECK-restatement hazard that the audit
-- event lookup table was created to escape.

-- An imported cmi5 package. `manifest_xml` is kept verbatim so export can
-- reproduce the original faithfully; `content_path` is the sub-directory of the
-- configured content dir the package was extracted to.
CREATE TABLE cmi5_courses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    course_iri TEXT NOT NULL,
    title TEXT,
    description TEXT,
    content_path TEXT NOT NULL,
    manifest_xml TEXT NOT NULL,
    imported_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

-- The block tree of a course. A block nests blocks and/or AUs; a multi-AU block
-- is simply a block whose AUs number more than one. `parent_block_id` is the
-- self-reference that builds the tree; top-level blocks have it NULL.
CREATE TABLE cmi5_blocks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    course_id UUID NOT NULL REFERENCES cmi5_courses(id) ON DELETE CASCADE,
    parent_block_id UUID REFERENCES cmi5_blocks(id) ON DELETE CASCADE,
    block_iri TEXT,
    title TEXT,
    position INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The assignable units. `training_step_id` is the integration point: an admin
-- binds an AU to a training step, and a server-verified pass of the AU then
-- writes that step's user_training_progress exactly as a trainer sign-off does,
-- so passing the course grants physical tool access. NULL until an admin maps
-- it; ON DELETE SET NULL so removing a step unbinds rather than deleting the AU.
CREATE TABLE cmi5_assignable_units (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    course_id UUID NOT NULL REFERENCES cmi5_courses(id) ON DELETE CASCADE,
    block_id UUID REFERENCES cmi5_blocks(id) ON DELETE CASCADE,
    au_iri TEXT NOT NULL,
    title TEXT,
    launch_url TEXT NOT NULL,
    launch_parameters TEXT,
    launch_method TEXT,
    move_on TEXT NOT NULL,
    mastery_score DOUBLE PRECISION,
    position INTEGER NOT NULL,
    training_step_id UUID REFERENCES training_steps(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (course_id, au_iri)
);

-- One learner attempt at one AU. `id` is the xAPI registration UUID carried in
-- the launch and echoed on every statement. The *_at columns record the first
-- time each outcome was observed. launch_mode gates crediting: only 'Normal'
-- may satisfy moveOn (Browse/Review are non-credit per cmi5).
CREATE TABLE cmi5_registrations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    au_id UUID NOT NULL REFERENCES cmi5_assignable_units(id) ON DELETE CASCADE,
    actor_account_name TEXT NOT NULL,
    launch_mode TEXT NOT NULL DEFAULT 'Normal',
    satisfied_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    passed_at TIMESTAMPTZ,
    failed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Launch credentials for a registration. Only SHA-256 hex hashes are stored:
-- the fetch token lives in the launch URL and the session token in the running
-- content, and a database read must not yield a working credential. The fetch
-- token is single-use (claimed atomically, like account_tokens); the session
-- token is what the content authenticates the LRS with until session_expires_at.
CREATE TABLE cmi5_launch_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    registration_id UUID NOT NULL REFERENCES cmi5_registrations(id) ON DELETE CASCADE,
    fetch_token_hash TEXT NOT NULL UNIQUE,
    session_token_hash TEXT UNIQUE,
    fetch_consumed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    session_expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The embedded-minimal LRS store. `statement_id` is the xAPI statement id and is
-- UNIQUE, which is what makes a replayed PUT idempotent rather than a second
-- grant. `statement` holds the full xAPI JSON as received.
CREATE TABLE cmi5_statements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    registration_id UUID NOT NULL REFERENCES cmi5_registrations(id) ON DELETE CASCADE,
    statement_id UUID NOT NULL UNIQUE,
    verb_iri TEXT NOT NULL,
    stored TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    statement JSONB NOT NULL,
    voided BOOLEAN NOT NULL DEFAULT FALSE
);

-- The xAPI State API document store. The server writes 'LMS.LaunchData' here at
-- launch for the content to read back. The four-column key is exactly the State
-- API's addressing (registration + activity + agent + stateId); all NOT NULL so
-- the uniqueness holds without NULL-distinctness surprises.
CREATE TABLE cmi5_state_documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    registration_id UUID NOT NULL REFERENCES cmi5_registrations(id) ON DELETE CASCADE,
    activity_iri TEXT NOT NULL,
    agent_account_name TEXT NOT NULL,
    state_id TEXT NOT NULL,
    document JSONB NOT NULL,
    etag TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (registration_id, activity_iri, agent_account_name, state_id)
);

-- Indexes for the hot lookups: course listings, tree walks, per-registration
-- statement/state reads, and the token claims.
CREATE INDEX idx_cmi5_courses_deleted_at ON cmi5_courses (deleted_at);
CREATE INDEX idx_cmi5_blocks_course_id ON cmi5_blocks (course_id);
CREATE INDEX idx_cmi5_blocks_parent_block_id ON cmi5_blocks (parent_block_id);
CREATE INDEX idx_cmi5_aus_course_id ON cmi5_assignable_units (course_id);
CREATE INDEX idx_cmi5_aus_block_id ON cmi5_assignable_units (block_id);
CREATE INDEX idx_cmi5_aus_training_step_id ON cmi5_assignable_units (training_step_id);
CREATE INDEX idx_cmi5_registrations_user_id ON cmi5_registrations (user_id);
CREATE INDEX idx_cmi5_registrations_au_id ON cmi5_registrations (au_id);
CREATE INDEX idx_cmi5_launch_tokens_registration_id ON cmi5_launch_tokens (registration_id);
CREATE INDEX idx_cmi5_statements_registration_id ON cmi5_statements (registration_id, stored);
CREATE INDEX idx_cmi5_state_documents_registration_id ON cmi5_state_documents (registration_id);

COMMENT ON TABLE cmi5_courses IS 'Imported cmi5 packages; manifest_xml kept verbatim for faithful export.';
COMMENT ON TABLE cmi5_blocks IS 'The block tree of a course; parent_block_id builds the hierarchy.';
COMMENT ON TABLE cmi5_assignable_units IS 'Launchable AUs; training_step_id binds an AU to physical tool access.';
COMMENT ON TABLE cmi5_registrations IS 'One learner attempt at one AU; id is the xAPI registration UUID.';
COMMENT ON TABLE cmi5_launch_tokens IS 'Single-use fetch + session credentials, stored only as SHA-256 hashes.';
COMMENT ON TABLE cmi5_statements IS 'Embedded LRS statement store; statement_id UNIQUE makes replays idempotent.';
COMMENT ON TABLE cmi5_state_documents IS 'xAPI State API documents, including the server-written LMS.LaunchData.';
