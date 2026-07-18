DROP INDEX IF EXISTS idx_tools_schedule_id;
ALTER TABLE tools     DROP COLUMN IF EXISTS schedule_id;
ALTER TABLE schedules DROP COLUMN IF EXISTS is_public;
