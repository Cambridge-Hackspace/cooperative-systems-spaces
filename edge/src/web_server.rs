use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tracing::{info, error};

#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;
#[cfg(not(debug_assertions))]
use axum_embed::ServeEmbed;

#[cfg(debug_assertions)]
use tower_http::services::ServeDir;

use crate::config::{Config, AuthStatus};
use crate::registration::register_device;
use crate::system_info::SystemInfo;

#[cfg(not(debug_assertions))]
#[derive(RustEmbed, Clone)]
#[folder = "../frontend_edge/dist"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: String,
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
pub async fn get_status(State(state): State<AppState>) -> Result<Json<ApiResponse<DeviceStatusResponse>>, StatusCode> {
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
    Json(req): Json<RegisterRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    let config = state.config.read().unwrap().clone();
    
    // Check if already registered
    if config.auth_status != AuthStatus::Unauthenticated {
        return Ok(Json(ApiResponse::error(
            "Device is already registered".to_string()
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
                        "Registration successful! Please restart the edge device.".to_string()
                    )))
                }
                Err(e) => {
                    error!("Failed to reload config after registration: {}", e);
                    Ok(Json(ApiResponse::success(
                        "Registration successful! Please restart the edge device.".to_string()
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
        .fallback(serve_embedded)
        .with_state(state)
}

/// Create the web server router (debug mode - serves from filesystem)
#[cfg(debug_assertions)]
pub fn create_router(state: AppState) -> Router {
    use tower_http::services::{ServeDir, ServeFile};
    
    let serve_dir = ServeDir::new("./frontend_edge/dist")
        .not_found_service(ServeFile::new("./frontend_edge/dist/index.html"));
    
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/register", post(register))
        .fallback_service(serve_dir)
        .with_state(state)
}

/// Start the web server
pub async fn start_web_server(
    config: Arc<RwLock<Config>>,
    config_path: String,
    port: u16,
) -> Result<()> {
    let state = AppState {
        config,
        config_path,
    };

    let app = create_router(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("Starting web server on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
