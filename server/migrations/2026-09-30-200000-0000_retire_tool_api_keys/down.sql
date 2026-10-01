-- Reverse of retire_tool_api_keys: restore the column and its index.
--
-- The values are NOT restored, and cannot be: they were secrets, and dropping the
-- column destroyed them. A deployment rolling back has to reissue per-tool keys.

ALTER TABLE tools ADD COLUMN external_api_key TEXT;

CREATE INDEX idx_tools_external_api_key ON tools(external_api_key) WHERE external_api_key IS NOT NULL;

COMMENT ON COLUMN tools.external_api_key IS 'Optional API key that can be used to authenticate ToolPass requests for this specific tool, overriding the global API key';
