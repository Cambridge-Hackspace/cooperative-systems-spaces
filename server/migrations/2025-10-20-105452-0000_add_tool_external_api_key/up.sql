-- Add external_api_key column to tools table
ALTER TABLE tools ADD COLUMN external_api_key TEXT;

-- Add index for external_api_key lookups
CREATE INDEX idx_tools_external_api_key ON tools(external_api_key) WHERE external_api_key IS NOT NULL;

-- Add comment
COMMENT ON COLUMN tools.external_api_key IS 'Optional API key that can be used to authenticate ToolPass requests for this specific tool, overriding the global API key';
