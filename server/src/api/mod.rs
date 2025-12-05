pub mod auth;
pub mod calendar;
pub mod config;
pub mod pages;
pub mod errors;
pub mod responses;
pub mod users;
pub mod admin;
pub mod profiles;
pub mod tools;
pub mod training;
pub mod trainers;
pub mod toolpass;
pub mod devices;

use axum::Router;
use crate::AppState;

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
        .nest("/toolpass", toolpass::toolpass_routes())
        .nest("/calendar", calendar::calendar_routes())
        .nest("/pages", pages::pages_routes())
        .nest("/devices", devices::devices_routes())
}
