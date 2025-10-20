-- Add external_id column to tools table for ToolPass device integration
ALTER TABLE tools ADD COLUMN external_id VARCHAR UNIQUE;

-- Add index for efficient lookups
CREATE INDEX idx_tools_external_id ON tools(external_id) WHERE external_id IS NOT NULL;

-- Add comment for documentation
COMMENT ON COLUMN tools.external_id IS 'External ID for third-party integrations like ToolPass devices';
