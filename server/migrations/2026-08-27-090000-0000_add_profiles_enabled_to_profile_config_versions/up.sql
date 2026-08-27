-- profiles_enabled travels with the versioned field schema so both are
-- persisted (and rolled back) together, instead of profiles_enabled being
-- written to the on-disk config file separately.
ALTER TABLE profile_config_versions ADD COLUMN profiles_enabled BOOLEAN NOT NULL DEFAULT TRUE;
