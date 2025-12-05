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
    config::{ToolCategoryMapping, LinkLocation},
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
pub struct PublicPagesConfig {
    pub wiki_enabled: bool,
    pub wiki_link: LinkLocation,
    pub site_enabled: bool,
    pub site_link: LinkLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicThemeConfig {
    pub primary_color: String,
    pub secondary_color: String,
    pub accent_color: String,
    pub background_color: String,
    pub text_color: String,
    pub logo_url: Option<String>,
    pub favicon_url: Option<String>,
    pub dark_mode_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicConfig {
    pub registration_challenge: PublicRegistrationChallengeConfig,
    pub tools: PublicToolConfig,
    pub site: PublicSiteConfig,
    pub pages: PublicPagesConfig,
    pub theme: PublicThemeConfig,
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
        pages: PublicPagesConfig {
            wiki_enabled: config.pages.wiki_repo.is_some(),
            wiki_link: config.pages.wiki_link.clone(),
            site_enabled: config.pages.site_repo.is_some(),
            site_link: config.pages.site_link.clone(),
        },
        theme: PublicThemeConfig {
            primary_color: config.theme.primary_color.clone(),
            secondary_color: config.theme.secondary_color.clone(),
            accent_color: config.theme.accent_color.clone(),
            background_color: config.theme.background_color.clone(),
            text_color: config.theme.text_color.clone(),
            logo_url: config.theme.logo_url.clone(),
            favicon_url: config.theme.favicon_url.clone(),
            dark_mode_enabled: config.theme.dark_mode_enabled,
        },
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
        pages: PublicPagesConfig {
            wiki_enabled: config.pages.wiki_repo.is_some(),
            wiki_link: config.pages.wiki_link.clone(),
            site_enabled: config.pages.site_repo.is_some(),
            site_link: config.pages.site_link.clone(),
        },
        theme: PublicThemeConfig {
            primary_color: config.theme.primary_color.clone(),
            secondary_color: config.theme.secondary_color.clone(),
            accent_color: config.theme.accent_color.clone(),
            background_color: config.theme.background_color.clone(),
            text_color: config.theme.text_color.clone(),
            logo_url: config.theme.logo_url.clone(),
            favicon_url: config.theme.favicon_url.clone(),
            dark_mode_enabled: config.theme.dark_mode_enabled,
        },
    };

    Ok(Json(ApiResponse::success(public_config)))
}
