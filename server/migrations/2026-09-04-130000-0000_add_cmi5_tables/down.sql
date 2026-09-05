-- Drop the cmi5 tables in FK-dependency order (children before parents).
DROP TABLE IF EXISTS cmi5_state_documents;
DROP TABLE IF EXISTS cmi5_statements;
DROP TABLE IF EXISTS cmi5_launch_tokens;
DROP TABLE IF EXISTS cmi5_registrations;
DROP TABLE IF EXISTS cmi5_assignable_units;
DROP TABLE IF EXISTS cmi5_blocks;
DROP TABLE IF EXISTS cmi5_courses;
