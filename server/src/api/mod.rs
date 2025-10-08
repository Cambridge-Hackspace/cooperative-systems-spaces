pub mod auth;
pub mod errors;
pub mod responses;
pub mod users;
pub mod admin;

use axum::Router;
use crate::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::auth_routes())
        .nest("/users", users::user_routes())
        .nest("/admin", admin::admin_routes())
}