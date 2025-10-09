-- Drop tables in reverse order due to foreign key constraints
DROP TABLE IF EXISTS tool_trainers;
DROP TABLE IF EXISTS user_tool_training;
DROP TABLE IF EXISTS tool_training_types;
DROP TABLE IF EXISTS tool_events;
DROP TABLE IF EXISTS tools;

-- Drop custom types
DROP TYPE IF EXISTS tool_status;
DROP TYPE IF EXISTS tool_category;