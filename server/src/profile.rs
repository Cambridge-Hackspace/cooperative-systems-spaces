use crate::{
    config::{ProfileFieldType, ProfileField},
    models::{AuditEventType, NewAuditLog},
    database::DatabaseManager,
};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// Profile validation service
#[derive(Clone)]
pub struct ProfileValidator {
    field_definitions: HashMap<String, ProfileField>,
}

impl ProfileValidator {
    /// Create a new profile validator from a field schema.
    pub fn new(profile_fields: &[ProfileField]) -> Self {
        let mut field_definitions = HashMap::new();
        for field in profile_fields {
            field_definitions.insert(field.key.clone(), field.clone());
        }

        Self { field_definitions }
    }

    /// Validate a complete profile object
    pub fn validate_profile(&self, profile: &Value) -> Result<()> {
        let profile_obj = profile.as_object()
            .ok_or_else(|| anyhow!("Profile must be a JSON object"))?;

        // Check for unknown fields
        for field_name in profile_obj.keys() {
            if !self.field_definitions.contains_key(field_name) {
                return Err(anyhow!("Unknown profile field: {}", field_name));
            }
        }

        // Validate each field according to its definition
        for (field_name, field_def) in &self.field_definitions {
            if let Some(field_value) = profile_obj.get(field_name) {
                self.validate_field(field_name, field_value, field_def)?;
            } else if field_def.required {
                return Err(anyhow!("Required field missing: {}", field_name));
            }
        }

        Ok(())
    }

    /// Validate a single field value
    fn validate_field(&self, field_name: &str, value: &Value, field_def: &ProfileField) -> Result<()> {
        match &field_def.field_type {
            ProfileFieldType::Text => {
                let _string_val = value.as_str()
                    .ok_or_else(|| anyhow!("Field '{}' must be a string", field_name))?;
            }

            ProfileFieldType::TextArray => {
                let arr = value.as_array().ok_or_else(|| {
                    anyhow!("Field '{}' must be an array of strings", field_name)
                })?;
                for (i, item) in arr.iter().enumerate() {
                    if !item.is_string() {
                        return Err(anyhow!(
                            "Field '{}' item at index {} must be a string",
                            field_name,
                            i
                        ));
                    }
                }
            }

            ProfileFieldType::Email => {
                let email_str = value.as_str()
                    .ok_or_else(|| anyhow!("Field '{}' must be a string", field_name))?;

                if !email_str.contains('@') || !email_str.contains('.') {
                    return Err(anyhow!("Field '{}' must be a valid email address", field_name));
                }
            }

            ProfileFieldType::Phone => {
                let phone_str = value.as_str()
                    .ok_or_else(|| anyhow!("Field '{}' must be a string", field_name))?;

                // Basic phone validation
                if phone_str.is_empty() || phone_str.len() < 7 {
                    return Err(anyhow!("Field '{}' must be a valid phone number", field_name));
                }
            }

            ProfileFieldType::Number => {
                let _number_val = value.as_f64()
                    .ok_or_else(|| anyhow!("Field '{}' must be a number", field_name))?;
            }

            ProfileFieldType::Date => {
                let date_str = value.as_str()
                    .ok_or_else(|| anyhow!("Field '{}' must be a date string", field_name))?;

                // Basic date format validation (YYYY-MM-DD)
                if !date_str.matches('-').count() == 2 || date_str.len() != 10 {
                    return Err(anyhow!("Field '{}' must be in YYYY-MM-DD format", field_name));
                }
            }

            ProfileFieldType::Boolean => {
                let _bool_val = value.as_bool()
                    .ok_or_else(|| anyhow!("Field '{}' must be a boolean", field_name))?;
            }

            ProfileFieldType::Select { options } => {
                let select_val = value.as_str()
                    .ok_or_else(|| anyhow!("Field '{}' must be a string", field_name))?;

                if !options.contains(&select_val.to_string()) {
                    return Err(anyhow!("Field '{}' must be one of: {:?}", field_name, options));
                }
            }
        }

        Ok(())
    }
}

/// Simple audit logging service
#[derive(Clone)]
pub struct AuditLogger {
    enabled: bool,
    db: DatabaseManager,
}

impl AuditLogger {
    pub fn new(db: std::sync::Arc<DatabaseManager>) -> Self {
        Self {
            enabled: true,
            db: (*db).clone(),
        }
    }

    /// Log an audit event to the database
    pub async fn log_event(
        &self,
        event_type: AuditEventType,
        user_id: Option<Uuid>,
        actor_id: Option<Uuid>,
        event_data: Value,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // Log to tracing for debugging
        tracing::info!(
            "Audit Event: {} - User: {:?} - Actor: {:?} - Data: {} - IP: {:?}",
            event_type.as_str(),
            user_id,
            actor_id,
            event_data,
            ip_address
        );

        // Create new audit log entry
        let audit_log = NewAuditLog {
            event_type: event_type.as_str().to_string(),
            user_id,
            actor_id,
            event_data,
            ip_address,
            user_agent,
        };

        // Save to database
        if let Err(e) = self.db.create_audit_log(&audit_log) {
            tracing::error!("Failed to save audit log to database: {}", e);
        }

        Ok(())
    }

    /// Log user registration event
    pub async fn log_user_registration(
        &self,
        user_id: Uuid,
        username: &str,
        email: &str,
        role: &str,
        ip_address: Option<String>,
        _user_agent: Option<String>,
    ) -> Result<()> {
        let event_data = serde_json::json!({
            "username": username,
            "email": email,
            "role": role,
            "action": "User registered successfully"
        });

        self.log_event(
            AuditEventType::UserRegistration,
            Some(user_id),
            None, // Self-registration has no actor
            event_data,
            ip_address,
            _user_agent,
        ).await
    }

    /// Log profile update event
    pub async fn log_profile_update(
        &self,
        user_id: Uuid,
        actor_id: Option<Uuid>,
        old_profile: &Value,
        new_profile: &Value,
        ip_address: Option<String>,
        _user_agent: Option<String>,
    ) -> Result<()> {
        let event_data = serde_json::json!({
            "old_profile": old_profile,
            "new_profile": new_profile,
            "action": "User profile updated"
        });

        self.log_event(
            AuditEventType::UserProfileUpdate,
            Some(user_id),
            actor_id.or(Some(user_id)), // If no actor, assume self-update
            event_data,
            ip_address,
            _user_agent,
        ).await
    }
}
