use anyhow::Result;
use axum::{
    extract::ws::{Message, WebSocket},
    extract::{ConnectInfo, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tracing::{error, info};

#[cfg(not(debug_assertions))]
#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

use crate::config::{AuthStatus, Config};
use crate::registration::register_device;
use crate::system_info::SystemInfo;
use crate::toolguard::ToolGuardState;

#[cfg(not(debug_assertions))]
#[derive(RustEmbed, Clone)]
#[folder = "../frontend_edge/dist"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: String,
    pub toolguard_state: Arc<ToolGuardState>,
    pub frontend_path: String,
    /// #120 (#123/H1): the per-process pairing token printed to the log at
    /// startup. A sensitive endpoint is served to a loopback client
    /// unconditionally, or to a remote client that presents this token.
    pub pairing_token: Arc<String>,
}

/// #120 (#123/H1): a sensitive local endpoint (`/api/register`, `/api/status`,
/// `/api/toolguard/state`) is reachable either from loopback -- the operator at
/// the device, and the default bind -- or by presenting the pairing token the
/// edge prints to its log at startup. Anything else is refused, so binding the
/// UI to the LAN does not thereby hand registration, device details, or the
/// member roster (PII + offline-attackable card digests) to that LAN.
fn local_or_token_ok(peer: SocketAddr, headers: &HeaderMap, token: &str) -> bool {
    if peer.ip().is_loopback() {
        return true;
    }
    headers
        .get("X-Pairing-Token")
        .and_then(|v| v.to_str().ok())
        .map(|presented| css_lib::ct::constant_time_str_eq(presented, token))
        .unwrap_or(false)
}

#[derive(Debug, Serialize)]
pub struct DeviceStatusResponse {
    pub device_name: String,
    pub is_registered: bool,
    pub auth_status: String,
    pub system_info: SystemInfo,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub instance_url: String,
    pub device_code: String,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}

/// GET /api/status - Get device status
pub async fn get_status(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<DeviceStatusResponse>>, StatusCode> {
    if !local_or_token_ok(peer, &headers, &state.pairing_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let config = state.config.read().unwrap();

    let system_info = SystemInfo::collect();

    let status = DeviceStatusResponse {
        device_name: config.name.clone(),
        is_registered: config.auth_status != AuthStatus::Unauthenticated,
        auth_status: match config.auth_status {
            AuthStatus::Unauthenticated => "unauthenticated".to_string(),
            AuthStatus::Pending => "pending".to_string(),
            AuthStatus::Approved => "approved".to_string(),
            AuthStatus::Denied => "denied".to_string(),
        },
        system_info,
    };

    Ok(Json(ApiResponse::success(status)))
}

/// POST /api/register - Register device
pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    if !local_or_token_ok(peer, &headers, &state.pairing_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let config = state.config.read().unwrap().clone();

    // Check if already registered
    if config.auth_status != AuthStatus::Unauthenticated {
        return Ok(Json(ApiResponse::error(
            "Device is already registered".to_string(),
        )));
    }

    drop(config);

    info!("Attempting to register device via web UI...");
    info!("Instance URL: {}", req.instance_url);

    // Get current config
    let current_config = state.config.read().unwrap().clone();

    // Perform registration
    match register_device(
        &req.instance_url,
        &req.device_code,
        &current_config,
        std::path::Path::new(&state.config_path),
    )
    .await
    {
        Ok(_) => {
            info!("Registration successful via web UI");

            // Reload config
            match crate::config::load_config(&state.config_path) {
                Ok(new_config) => {
                    *state.config.write().unwrap() = new_config;
                    Ok(Json(ApiResponse::success(
                        "Registration successful! Please restart the edge apparatus.".to_string(),
                    )))
                }
                Err(e) => {
                    error!("Failed to reload config after registration: {}", e);
                    Ok(Json(ApiResponse::success(
                        "Registration successful! Please restart the edge apparatus.".to_string(),
                    )))
                }
            }
        }
        Err(e) => {
            error!("Registration failed: {}", e);
            Ok(Json(ApiResponse::error(format!(
                "Registration failed: {}",
                e
            ))))
        }
    }
}

/// GET /api/toolguard/ws - WebSocket endpoint that pushes state on every change
pub async fn toolguard_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // #120 (#123/H1): the ws pushes the full toolguard payload on connect, so it
    // is gated exactly like GET /api/toolguard/state.
    if !local_or_token_ok(peer, &headers, &state.pairing_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| handle_toolguard_ws(socket, state))
        .into_response()
}

async fn handle_toolguard_ws(mut socket: WebSocket, state: AppState) {
    // Send current state immediately on connect
    if let Some(payload) = state.toolguard_state.get_state() {
        if let Ok(json) = serde_json::to_string(&payload) {
            if socket.send(Message::Text(json.into())).await.is_err() {
                return;
            }
        }
    }

    let mut rx = state.toolguard_state.subscribe_ws();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(json) => {
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

/// GET /api/toolguard/state - Return the current locally-cached toolguard sync payload
pub async fn get_toolguard_state(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<crate::toolguard::SyncPayload>>, StatusCode> {
    if !local_or_token_ok(peer, &headers, &state.pairing_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    match state.toolguard_state.get_state() {
        Some(payload) => Ok(Json(ApiResponse::success(payload))),
        None => Ok(Json(ApiResponse::error(
            "No toolguard state available yet".to_string(),
        ))),
    }
}

/// Create the web server router
#[cfg(not(debug_assertions))]
pub fn create_router(state: AppState) -> Router {
    use axum::http::{header, Uri};
    use axum::response::{IntoResponse, Response};

    async fn serve_embedded(uri: Uri) -> impl IntoResponse {
        let path = uri.path().trim_start_matches('/');

        // Try to get the file from embedded assets
        match Assets::get(path) {
            Some(content) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                let body = axum::body::Body::from(content.data.to_vec());
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .body(body)
                    .unwrap()
            }
            None => {
                // If not found, try to serve index.html (for SPA routing)
                match Assets::get("index.html") {
                    Some(content) => {
                        let body = axum::body::Body::from(content.data.to_vec());
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/html")
                            .body(body)
                            .unwrap()
                    }
                    None => {
                        let body = axum::body::Body::from("404 Not Found");
                        Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(body)
                            .unwrap()
                    }
                }
            }
        }
    }

    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/register", post(register))
        .route("/api/toolguard/state", get(get_toolguard_state))
        .route("/api/toolguard/ws", get(toolguard_ws))
        .fallback(serve_embedded)
        .with_state(state)
}

/// Create the web server router (debug mode - serves from filesystem)
#[cfg(debug_assertions)]
pub fn create_router(state: AppState) -> Router {
    use tower_http::services::{ServeDir, ServeFile};

    // `fallback`, not `not_found_service` -- see the note in server/src/main.rs.
    // `not_found_service` overrides the status to 404, so every client-side
    // route was served the right page with the wrong status.
    let serve_dir = ServeDir::new(&state.frontend_path).fallback(ServeFile::new(format!(
        "{}/index.html",
        state.frontend_path
    )));

    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/register", post(register))
        .route("/api/toolguard/state", get(get_toolguard_state))
        .route("/api/toolguard/ws", get(toolguard_ws))
        .fallback_service(serve_dir)
        .with_state(state)
}

/// Start the web server
pub async fn start_web_server(
    config: Arc<RwLock<Config>>,
    config_path: String,
    port: u16,
    toolguard_state: Arc<ToolGuardState>,
    frontend_path: String,
    pairing_token: Arc<String>,
) -> Result<()> {
    // #120 (#123/H1): bind loopback by default (config-overridable) instead of
    // 0.0.0.0, and log the address actually bound rather than a hard-coded
    // "localhost" that used to be a falsehood.
    let bind_host = config.read().unwrap().web_ui_bind_address.clone();

    let state = AppState {
        config,
        config_path,
        toolguard_state,
        frontend_path,
        pairing_token,
    };

    let app = create_router(state);

    let addr = format!("{}:{}", bind_host, port);
    info!("Starting web server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    // into_make_service_with_connect_info: handlers need the peer address to
    // enforce the loopback-or-token rule (#123/H1).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn hdrs(token: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(t) = token {
            h.insert("X-Pairing-Token", t.parse().unwrap());
        }
        h
    }

    // #120 (#123/H1): loopback is exempt (the operator at the device), so the
    // default-bound UI keeps working with no token.
    #[test]
    fn loopback_is_allowed_without_a_token() {
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
        assert!(local_or_token_ok(peer, &hdrs(None), "secret"));
        // A wrong token from loopback is still fine -- loopback alone suffices.
        assert!(local_or_token_ok(peer, &hdrs(Some("wrong")), "secret"));
    }

    // A non-loopback client is refused unless it presents the exact token.
    #[test]
    fn a_remote_client_needs_the_right_token() {
        let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), 40000);
        assert!(!local_or_token_ok(peer, &hdrs(None), "secret"));
        assert!(!local_or_token_ok(peer, &hdrs(Some("wrong")), "secret"));
        assert!(local_or_token_ok(peer, &hdrs(Some("secret")), "secret"));
    }
}
