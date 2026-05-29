-- Two small extensions on top of the schedules feature:
--   * `tools.schedule_id` lets a schedule gate when a tool can be used
--     (closed schedule = same effect as the tool being deactivated).
--   * `schedules.is_public` lets an admin opt a schedule into the public
--     "open today" surfacing on the member-facing home page without
--     exposing every internal schedule.

ALTER TABLE tools
    ADD COLUMN schedule_id UUID REFERENCES schedules(id) ON DELETE SET NULL;
CREATE INDEX idx_tools_schedule_id ON tools(schedule_id);

ALTER TABLE schedules
    ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT FALSE;
COMMENT ON COLUMN schedules.is_public IS
    'When true: surface this schedule on the public home page "Hours today" card.';
