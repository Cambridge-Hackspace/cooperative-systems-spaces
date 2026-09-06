use axum::{extract::State, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    config::{LinkLocation, MembershipPeriod, ToolCategoryMapping},
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
pub struct PublicHomepageLinksConfig {
    pub view_my_profile: bool,
    pub browse_tools: bool,
    pub admin_panel: bool,
    pub wiki: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSiteConfig {
    pub site_name: String,
    /// Org abbreviation for compact labels; empty falls back to `site_name`.
    pub site_short_name: String,
    pub homepage_links: PublicHomepageLinksConfig,
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
pub struct PublicDoorsConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicCalendarConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicGroupsioConfig {
    pub enabled: bool,
}

/// Membership facts the SPA needs to render the billing card. No secrets: the
/// checkout/portal flow mints hosted URLs server-side, so not even a publishable
/// key is exposed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicMembershipConfig {
    pub enabled: bool,
    /// Whether online (Stripe) payment is available on top of the module.
    pub stripe_enabled: bool,
    pub currency: String,
    pub due_amount: String,
    pub due_period: MembershipPeriod,
    pub plan_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicToolGuardConfig {
    pub enabled: bool,
    /// Name of the profile field that holds the user's RFID/NFC card id —
    /// clients need this to know where to write a scanned UID.
    pub profile_field: String,
}

/// The authentication facts a signed-out visitor legitimately needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicAuthConfig {
    /// Whether to offer account recovery at all.
    ///
    /// Both halves, ANDed. `password_reset_enabled` is the operator's intent
    /// and `email.enabled` is whether it can be carried out; the shipped
    /// defaults are true and false respectively, so the pair out of the box is
    /// "wanted but impossible". Sending the raw flag would put a link on the
    /// login page to an endpoint that answers 403 -- the same promise without a
    /// product that this whole feature exists to stop making.
    pub password_reset_enabled: bool,
    /// Whether a new account must confirm its address before signing in, so
    /// the registration page can say so before somebody waits for a login that
    /// will not work.
    pub require_email_verification: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicConfig {
    pub auth: PublicAuthConfig,
    pub registration_challenge: PublicRegistrationChallengeConfig,
    pub tools: PublicToolConfig,
    pub site: PublicSiteConfig,
    pub pages: PublicPagesConfig,
    pub theme: PublicThemeConfig,
    pub doors: PublicDoorsConfig,
    pub calendar: PublicCalendarConfig,
    pub toolguard: PublicToolGuardConfig,
    pub groupsio: PublicGroupsioConfig,
    pub membership: PublicMembershipConfig,
}

fn build_public_config(state: &AppState) -> PublicConfig {
    let config = state.config_manager.get_config();
    PublicConfig {
        auth: PublicAuthConfig {
            password_reset_enabled: config.auth.password_reset_enabled && config.email.enabled,
            require_email_verification: config.auth.require_email_verification,
        },
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
        site: PublicSiteConfig {
            site_name: config.site.site_name.clone(),
            site_short_name: config.site.site_short_name.clone(),
            homepage_links: PublicHomepageLinksConfig {
                view_my_profile: config.site.homepage_links.view_my_profile,
                browse_tools: config.site.homepage_links.browse_tools,
                admin_panel: config.site.homepage_links.admin_panel,
                wiki: config.site.homepage_links.wiki,
            },
        },
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
        doors: PublicDoorsConfig {
            enabled: config.door.enabled,
        },
        calendar: PublicCalendarConfig {
            enabled: config.calendar.enabled,
        },
        toolguard: PublicToolGuardConfig {
            enabled: config.toolguard.enabled,
            profile_field: config.toolguard.profile_field.clone(),
        },
        groupsio: PublicGroupsioConfig {
            enabled: config.groupsio.enabled,
        },
        membership: PublicMembershipConfig {
            enabled: config.membership.enabled,
            stripe_enabled: config.stripe.enabled,
            currency: config.membership.currency.clone(),
            due_amount: config.membership.due_amount.clone(),
            due_period: config.membership.due_period,
            plan_name: config.membership.plan_name.clone(),
        },
    }
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
    Ok(Json(ApiResponse::success(build_public_config(&state))))
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
    Ok(Json(ApiResponse::success(build_public_config(&state))))
}
