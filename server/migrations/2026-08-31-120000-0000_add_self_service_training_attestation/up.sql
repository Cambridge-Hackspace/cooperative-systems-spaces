-- Self-service training attestation.
--
-- Issue #2 asks for a binding "I have read this safety documentation"
-- checkbox, so that there is a record. Two columns and one event type.
--
-- self_attestable marks a step whose completion is the trainee's own act
-- rather than a trainer's sign-off. Default false: every step that exists
-- today was signed off by staff or a certified instructor, and silently
-- turning any of them into something a member can grant themselves would
-- be the opposite of what this is for.
ALTER TABLE training_steps
    ADD COLUMN self_attestable BOOLEAN NOT NULL DEFAULT false;

-- What the member actually agreed to, captured when they agree to it.
--
-- training_steps.training_materials_url is mutable through PUT /steps/{id}.
-- Without a snapshot, editing the link -- or replacing the page behind it --
-- silently rewrites what every prior attestation was an attestation to, which
-- is the one property a record kept for legal reasons cannot afford to lose.
--
-- Nullable, no default: rows that predate this column were completed before
-- anything was captured, and inventing a URL for them would be worse than
-- admitting none was recorded.
ALTER TABLE user_training_progress
    ADD COLUMN acknowledged_materials_url TEXT;
