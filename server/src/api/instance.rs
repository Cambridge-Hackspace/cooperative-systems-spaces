use axum::{
    extract::State,
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;

use crate::{
    api::{errors::ApiError, responses::ApiResponse},
    auth::AuthUser,
    AppState,
};

/// Payload encoded into the instance QR for cross-device handoff. The Android
/// app and any future client parse this in their first-launch onboarding flow.
#[derive(Debug, Serialize)]
pub struct InstanceQrPayload {
    /// Payload schema version. Bump when fields are added/changed.
    pub v: u32,
    /// Canonical site URL clients should hit.
    pub url: String,
    /// Human-readable instance name (for display in client UIs).
    pub name: String,
}

pub fn instance_routes() -> Router<AppState> {
    Router::new().route("/qr", get(get_instance_qr))
}

/// GET /api/instance/qr — returns the JSON payload clients should encode into
/// a QR (or render directly) so another device can be onboarded to this same
/// instance. Available to any authenticated user — the payload contains only
/// the site URL and display name, both of which are non-sensitive.
async fn get_instance_qr(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<InstanceQrPayload>>, ApiError> {
    let cfg = state.config_manager.get_config();
    let payload = InstanceQrPayload {
        v: 1,
        url: cfg.site.site_url.trim_end_matches('/').to_string(),
        name: cfg.site.site_name.clone(),
    };
    Ok(Json(ApiResponse::success(payload)))
}
