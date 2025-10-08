use axum::{
    extract::State,
    response::Json,
    routing::{post},
    Router,
};

use crate::{
    api::{
        errors::ApiError,
        responses::ApiResponse,
    },
    auth::AdminUser,
    AppState,
};

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/reload-config", post(reload_config))
}

/// Reload configuration from disk (admin only)
async fn reload_config(
    _admin_user: AdminUser, // Ensures only admin users can access
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    // Attempt to reload the configuration
    match state.config.reload_config() {
        Ok(()) => {
            let new_config = state.config.get_config();
            Ok(Json(ApiResponse::success_with_message(
                serde_json::json!({
                    "site_name": new_config.site.site_name,
                    "debug_mode": new_config.site.debug,
                    "initial_setup_enabled": new_config.initial_setup.setup_enabled,
                    "auth_config": {
                        "allow_registration": new_config.auth.allow_registration,
                        "require_email_verification": new_config.auth.require_email_verification,
                        "password_min_length": new_config.auth.password_min_length
                    }
                }),
                "Configuration reloaded successfully".to_string(),
            )))
        }
        Err(e) => {
            tracing::error!("Failed to reload configuration: {}", e);
            Err(ApiError::InternalServerError(
                format!("Failed to reload configuration: {}", e)
            ))
        }
    }
}