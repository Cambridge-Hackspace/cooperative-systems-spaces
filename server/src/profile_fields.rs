use crate::config::ProfileField;

/// Validate a submitted set of profile field definitions: no empty keys or
/// labels, and no duplicate keys. Shared by the admin config-update
/// endpoint and the startup bootstrap that seeds the field schema from
/// `config.toml`, so a malformed schema can't reach the database either way.
pub fn validate_profile_fields(fields: &[ProfileField]) -> Result<(), String> {
    for field in fields {
        if field.key.is_empty() {
            return Err("Profile field key cannot be empty".to_string());
        }
        if field.label.is_empty() {
            return Err("Profile field label cannot be empty".to_string());
        }
    }

    let mut keys = std::collections::HashSet::new();
    for field in fields {
        if !keys.insert(&field.key) {
            return Err(format!("Duplicate field key: {}", field.key));
        }
    }

    Ok(())
}
