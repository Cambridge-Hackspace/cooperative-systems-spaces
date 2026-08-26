pub mod admin;
pub mod auth;
pub mod calendar;
pub mod config;
pub mod devices;
pub mod doors;
pub mod errors;
pub mod home_links;
pub mod instance;
pub mod mfa;
pub mod pages;
pub mod places;
pub mod profiles;
pub mod responses;
pub mod schedules;
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
}
