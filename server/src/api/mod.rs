pub mod admin;
pub mod auth;
pub mod calendar;
pub mod config;
pub mod devices;
pub mod doors;
pub mod errors;
pub mod groupsio;
pub mod home_links;
pub mod instance;
pub mod membership;
pub mod mfa;
pub mod pages;
pub mod places;
pub mod profiles;
pub mod responses;
pub mod schedules;
pub mod stripe;
pub mod toolguard;
pub mod tools;
pub mod trainers;
pub mod training;
pub mod users;
pub mod webhooks;

use crate::AppState;
use axum::Router;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::auth_routes())
        .nest("/users", users::user_routes())
        .nest("/admin", admin::admin_routes())
        .nest("/profiles", profiles::profile_routes())
        .nest("/tools", tools::tools_routes())
        .nest("/training", training::training_router())
        .nest("/trainers", trainers::trainers_router())
        .nest("/config", config::config_routes())
        .nest("/toolguard", toolguard::toolguard_routes())
        .nest("/calendar", calendar::calendar_routes())
        .nest("/pages", pages::pages_routes())
        .nest("/devices", devices::devices_routes())
        .nest("/doors", doors::member_routes())
        .nest("/places", places::member_routes())
        .nest("/schedules", schedules::member_routes())
        .nest("/public", schedules::public_routes())
        .nest("/public", home_links::public_routes())
        .nest("/instance", instance::instance_routes())
        .nest("/groupsio", groupsio::routes())
        .nest("/membership", membership::routes())
        .nest("/stripe", stripe::routes())
        // The API's own 404, and it belongs here rather than at the composition
        // site.
        //
        // `main.rs` mounts this router under a `fallback_service` that serves
        // the single-page application. Without a fallback of its own, an
        // unmatched /api path falls through to that -- so `/api/typo` answers
        // 200 with index.html, and a frontend calling a route that does not
        // exist gets a successful-looking HTML response whose `data` is a
        // string of markup that fails to destructure somewhere far away.
        //
        // Here rather than in main.rs because the contract tier builds its
        // router from this function. A fallback added at the composition site
        // would be absent from the 991-pair matrix, which asserts
        // `assert_ne!(status, 404)` on every route precisely because a mistyped
        // path 404s uniformly and looks reassuringly consistent -- an assertion
        // that means nothing if the router under test answers differently from
        // the one that ships.
        .fallback(api_not_found)
}

/// The 404 for an unmatched path under `/api`.
///
/// Answers in the same envelope as every other error. A mistyped endpoint is
/// the case most likely to be hit by code rather than by a person, so it is the
/// one where a consistent shape matters most.
async fn api_not_found(uri: axum::http::Uri) -> errors::ApiError {
    errors::ApiError::NotFound(format!("No such endpoint: {}", uri.path()))
}
