use axum::{
    extract::State,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        errors::ApiError,
        responses::ApiResponse,
    },
    config::ToolCategoryMapping,
    AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicRegistrationChallengeConfig {
    pub enabled: bool,
    pub hint: String,
    pub throttle_enabled: bool,
    pub terms_of_service_checkbox: bool,
    pub terms_of_service_md: String,
    pub recaptcha_enabled: bool,
    pub recaptcha_site_key: String,
    // Note: We don't expose the actual phrase, max attempts, lockout duration, or reCAPTCHA secret key for security
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicToolConfig {
    pub tool_categories: Vec<ToolCategoryMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSiteConfig {
    pub site_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicConfig {
    pub registration_challenge: PublicRegistrationChallengeConfig,
    pub tools: PublicToolConfig,
    pub site: PublicSiteConfig,
}

pub fn config_routes() -> Router<AppState> {
    Router::new()
        .route("/registration", get(get_registration_config))
        .route("/tools", get(get_tools_config))
        .route("/public", get(get_public_config))
}

/// Get public registration configuration
async fn get_registration_config(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<PublicConfig>>, ApiError> {
    let config = state.config_manager.get_config();
    let public_config = PublicConfig {
        registration_challenge: PublicRegistrationChallengeConfig {
            enabled: config.registration_challenge.enabled,
            hint: config.registration_challenge.hint.clone(),
            throttle_enabled: config.registration_challenge.throttle_enabled,
            terms_of_service_checkbox: config.registration_challenge.terms_of_service_checkbox,
            terms_of_service_md: config.registration_challenge.terms_of_service_md.clone(),
            recaptcha_enabled: config.registration_challenge.recaptcha_enabled,
            recaptcha_site_key: config.registration_challenge.recaptcha_site_key.clone(),
        },
        tools: PublicToolConfig {
            tool_categories: config.tools.tool_categories.clone(),
        },
        site: PublicSiteConfig { site_name: config.site.site_name.clone() },
    };

    Ok(Json(ApiResponse::success(public_config)))
}

/// Get public tools configuration
async fn get_tools_config(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<PublicToolConfig>>, ApiError> {
    let config = state.config_manager.get_config();
    let tools_config = PublicToolConfig {
        tool_categories: config.tools.tool_categories.clone(),
    };

    Ok(Json(ApiResponse::success(tools_config)))
}

/// Get all public configuration
async fn get_public_config(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<PublicConfig>>, ApiError> {
    let config = state.config_manager.get_config();
    let public_config = PublicConfig {
        registration_challenge: PublicRegistrationChallengeConfig {
            enabled: config.registration_challenge.enabled,
            hint: config.registration_challenge.hint.clone(),
            throttle_enabled: config.registration_challenge.throttle_enabled,
            terms_of_service_checkbox: config.registration_challenge.terms_of_service_checkbox,
            terms_of_service_md: config.registration_challenge.terms_of_service_md.clone(),
            recaptcha_enabled: config.registration_challenge.recaptcha_enabled,
            recaptcha_site_key: config.registration_challenge.recaptcha_site_key.clone(),
        },
        tools: PublicToolConfig {
            tool_categories: config.tools.tool_categories.clone(),
        },
        site: PublicSiteConfig { site_name: config.site.site_name },
    };

    Ok(Json(ApiResponse::success(public_config)))
}
