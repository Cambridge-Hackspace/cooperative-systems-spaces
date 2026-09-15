-- Reverse of the additive migration: drop the interlocks first (they reference
-- tool_modules via source_module_id), then the module bindings.
DROP TABLE IF EXISTS tool_interlocks;
DROP TABLE IF EXISTS tool_modules;
