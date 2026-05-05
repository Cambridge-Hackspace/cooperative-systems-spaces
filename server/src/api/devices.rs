use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AdminUser;
use crate::models::{
    AuditEventType, NewAuditLog, NewSpaceDevice, NewSpaceDeviceAuth, NewSpaceDeviceAuthRequest,
    SpaceDevice, SpaceDeviceAuthRequest, UpdateSpaceDevice,
};
use crate::schema::{space_device_auth, space_device_auth_requests, space_devices};
use crate::AppState;

use super::errors::ApiError;
use super::responses::ApiResponse;

/// Request to create a new device invite code
#[derive(Debug, Deserialize)]
pub struct CreateDeviceInviteRequest {
    /// Optional expiry duration in hours (default: 24)
    pub expires_in_hours: Option<i64>,
}

/// Response containing the device invite code
#[derive(Debug, Serialize)]
pub struct DeviceInviteResponse {
    pub device_code: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Request to register a device using an invite code
#[derive(Debug, Deserialize)]
pub struct RegisterDeviceRequest {
    pub device_code: String,
    pub name: String,
    pub kind: String, // "edge" or "kiosk"
    pub mac_address: String,
    pub software_version: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub platform: String, // "windows", "linux", "macos", "other"
}

/// Response after successful device registration
#[derive(Debug, Serialize)]
pub struct RegisterDeviceResponse {
    pub device_id: Uuid,
    pub auth_token: String,
    pub device_name: String,
    pub mqtt_config: Option<EdgeMqttConfig>,
}

/// MQTT configuration for edge apparatuss
#[derive(Debug, Serialize)]
pub struct EdgeMqttConfig {
    pub mqtt_instance_url: String,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub mqtt_namespace: String,
}

/// Device list item for admin
#[derive(Debug, Serialize)]
pub struct DeviceListItem {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub platform: String,
    pub mac_address: String,
    pub software_version: String,
    pub ipv4_address: Option<String>,
    pub ipv6_address: Option<String>,
    pub last_seen_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
    pub is_online: bool,
}

/// Device invite code list item
#[derive(Debug, Serialize)]
pub struct DeviceInviteListItem {
    pub id: Uuid,
    pub device_code: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub used_at: Option<chrono::DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: chrono::DateTime<Utc>,
    pub is_expired: bool,
    pub is_used: bool,
}

/// POST /api/admin/devices/invite - Generate a new device invite code
pub async fn create_device_invite(
    State(state): State<AppState>,
    admin: AdminUser,
    Json(req): Json<CreateDeviceInviteRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!("create_device_invite called by admin user: {}", admin.0.id);

    let expires_in_hours = req.expires_in_hours.unwrap_or(24);
    let expires_at = Utc::now() + Duration::hours(expires_in_hours);
    let device_code = SpaceDeviceAuthRequest::new_device_code();

    let conn = &mut state.db.pool().get().map_err(|e| {
        tracing::error!("Failed to get database connection: {}", e);
        ApiError::from(e)
    })?;

    // Create the invite request
    let new_invite = NewSpaceDeviceAuthRequest {
        device_code: device_code.clone(),
        expires_at,
        created_by: Some(admin.0.id),
    };

    let invite: SpaceDeviceAuthRequest = diesel::insert_into(space_device_auth_requests::table)
        .values(&new_invite)
        .get_result(conn)
        .map_err(|e| {
            tracing::error!("Failed to insert device invite: {}", e);
            ApiError::from(e)
        })?;

    // Create audit log
    let audit_log = NewAuditLog {
        event_type: AuditEventType::DeviceInviteCreated.as_str().to_string(),
        user_id: None,
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "device_code": device_code,
            "expires_at": expires_at,
            "expires_in_hours": expires_in_hours,
        }),
        ip_address: None,
        user_agent: None,
    };

    state.db.create_audit_log(&audit_log)?;

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(DeviceInviteResponse {
            device_code: invite.device_code,
            expires_at: invite.expires_at,
        })),
    ))
}

/// POST /api/devices/register - Register a device with an invite code
pub async fn register_device(
    State(state): State<AppState>,
    Json(req): Json<RegisterDeviceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!("register_device called for device: {} ({})", req.name, req.kind);

    let conn = &mut state.db.pool().get().map_err(|e| {
        tracing::error!("Failed to get database connection: {}", e);
        ApiError::from(e)
    })?;

    // Find and validate the invite code
    let invite: SpaceDeviceAuthRequest = space_device_auth_requests::table
        .filter(space_device_auth_requests::device_code.eq(&req.device_code))
        .first(conn)
        .map_err(|_| ApiError::BadRequest("Invalid device code".to_string()))?;

    // Check if already used
    if invite.used_at.is_some() {
        return Err(ApiError::BadRequest(
            "Device code has already been used".to_string(),
        ));
    }

    // Check if expired
    if invite.expires_at < Utc::now() {
        return Err(ApiError::BadRequest("Device code has expired".to_string()));
    }

    // Parse kind and platform
    let kind = match req.kind.to_lowercase().as_str() {
        "edge" => crate::models::SpaceDeviceKind::Edge,
        "kiosk" => crate::models::SpaceDeviceKind::Kiosk,
        _ => return Err(ApiError::BadRequest("Invalid device kind".to_string())),
    };

    let platform = match req.platform.to_lowercase().as_str() {
        "windows" => crate::models::SpaceDevicePlatform::Windows,
        "linux" => crate::models::SpaceDevicePlatform::Linux,
        "macos" => crate::models::SpaceDevicePlatform::MacOs,
        "other" => crate::models::SpaceDevicePlatform::Other,
        _ => return Err(ApiError::BadRequest("Invalid platform".to_string())),
    };

    // Create the device
    let new_device = NewSpaceDevice {
        name: req.name.clone(),
        kind,
        mac_address: req.mac_address,
        software_version: req.software_version,
        ipv4_address: req.ipv4_address,
        ipv6_address: req.ipv6_address,
        uptime: 0,
        platform,
    };

    let device: SpaceDevice = diesel::insert_into(space_devices::table)
        .values(&new_device)
        .get_result(conn)?;

    // Generate auth token (using UUID for now, could use JWT or other token format)
    let auth_token = Uuid::new_v4().to_string();

    // Create device auth
    let new_auth = NewSpaceDeviceAuth {
        device_id: device.id,
        auth_token: auth_token.clone(),
    };

    diesel::insert_into(space_device_auth::table)
        .values(&new_auth)
        .execute(conn)?;

    // Mark invite as used
    diesel::update(space_device_auth_requests::table)
        .filter(space_device_auth_requests::id.eq(invite.id))
        .set(space_device_auth_requests::used_at.eq(Some(Utc::now())))
        .execute(conn)?;

    // Create audit logs
    let invite_used_log = NewAuditLog {
        event_type: AuditEventType::DeviceInviteUsed.as_str().to_string(),
        user_id: None,
        actor_id: None,
        event_data: serde_json::json!({
            "device_code": req.device_code,
            "device_id": device.id,
            "device_name": device.name,
        }),
        ip_address: None,
        user_agent: None,
    };

    let device_registered_log = NewAuditLog {
        event_type: AuditEventType::DeviceRegistered.as_str().to_string(),
        user_id: None,
        actor_id: None,
        event_data: serde_json::json!({
            "device_id": device.id,
            "device_name": device.name,
            "kind": req.kind,
            "platform": req.platform,
        }),
        ip_address: None,
        user_agent: None,
    };

    // Log audit events (ignore errors)
    let _ = state.db.create_audit_log(&invite_used_log);
    let _ = state.db.create_audit_log(&device_registered_log);

    // Get MQTT configuration from server config
    let config = state.config_manager.get_config();
    
    tracing::info!("Checking MQTT config availability: edge_enabled={}, edge_mqtt_config={:?}", 
        config.edge.edge_enabled,
        config.edge.edge_mqtt_config.is_some()
    );
    
    let mqtt_config = config.edge.edge_mqtt_config.map(|mqtt| {
        tracing::info!("Providing MQTT config to device: url={}", mqtt.mqtt_instance_url);
        EdgeMqttConfig {
            mqtt_instance_url: mqtt.mqtt_instance_url,
            mqtt_username: mqtt.mqtt_username,
            mqtt_password: mqtt.mqtt_password,
            mqtt_namespace: mqtt.mqtt_namespace,
        }
    });
    
    if mqtt_config.is_none() {
        tracing::warn!("No MQTT configuration found in server config for edge apparatuss");
    }

    Ok((
        StatusCode::CREATED,
        Json(ApiResponse::success(RegisterDeviceResponse {
            device_id: device.id,
            auth_token,
            device_name: device.name,
            mqtt_config,
        })),
    ))
}

/// GET /api/admin/devices - List all devices
#[axum::debug_handler]
pub async fn list_devices(
    State(state): State<AppState>,
    admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!("list_devices called by admin user: {}", admin.0.id);

    let conn = &mut state.db.pool().get().map_err(|e| {
        tracing::error!("Failed to get database connection in list_devices: {}", e);
        ApiError::from(e)
    })?;

    let devices: Vec<SpaceDevice> = space_devices::table
        .filter(space_devices::deleted_at.is_null())
        .order(space_devices::created_at.desc())
        .load(conn)
        .map_err(|e| {
            tracing::error!("Failed to load devices from database: {}", e);
            ApiError::from(e)
        })?;

    tracing::debug!("Loaded {} devices from database", devices.len());

    let now = Utc::now();
    let offline_threshold = now - Duration::minutes(5); // 5 minutes threshold

    let device_list: Vec<DeviceListItem> = devices
        .into_iter()
        .map(|d| {
            let is_online = d
                .last_seen_at
                .map(|last_seen| last_seen > offline_threshold)
                .unwrap_or(false);

            DeviceListItem {
                id: d.id,
                name: d.name,
                kind: format!("{:?}", d.kind).to_lowercase(),
                platform: format!("{:?}", d.platform).to_lowercase(),
                mac_address: d.mac_address,
                software_version: d.software_version,
                ipv4_address: d.ipv4_address,
                ipv6_address: d.ipv6_address,
                last_seen_at: d.last_seen_at,
                created_at: d.created_at,
                is_online,
            }
        })
        .collect();

    Ok(Json(ApiResponse::success(device_list)))
}

/// GET /api/admin/devices/invites - List all device invite codes
pub async fn list_device_invites(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<impl IntoResponse, ApiError> {
    let conn = &mut state.db.pool().get()?;

    let invites: Vec<SpaceDeviceAuthRequest> = space_device_auth_requests::table
        .order(space_device_auth_requests::created_at.desc())
        .load(conn)?;

    let now = Utc::now();

    let invite_list: Vec<DeviceInviteListItem> = invites
        .into_iter()
        .map(|inv| DeviceInviteListItem {
            id: inv.id,
            device_code: inv.device_code,
            expires_at: inv.expires_at,
            used_at: inv.used_at,
            created_by: inv.created_by,
            created_at: inv.created_at,
            is_expired: inv.expires_at < now,
            is_used: inv.used_at.is_some(),
        })
        .collect();

    Ok(Json(ApiResponse::success(invite_list)))
}

/// DELETE /api/admin/devices/invites/{code} - Expire a device invite code
pub async fn expire_device_invite(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let conn = &mut state.db.pool().get()?;

    // Update the invite to expire it
    let updated = diesel::update(space_device_auth_requests::table)
        .filter(space_device_auth_requests::device_code.eq(&code))
        .filter(space_device_auth_requests::used_at.is_null())
        .set(space_device_auth_requests::expires_at.eq(Utc::now()))
        .execute(conn)?;

    if updated == 0 {
        return Err(ApiError::NotFound("Device invite not found or already used".to_string()));
    }

    // Create audit log
    let audit_log = NewAuditLog {
        event_type: AuditEventType::DeviceInviteExpired.as_str().to_string(),
        user_id: None,
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "device_code": code,
        }),
        ip_address: None,
        user_agent: None,
    };

    let _ = state.db.create_audit_log(&audit_log);

    Ok((
        StatusCode::OK,
        Json(ApiResponse::success_with_message(
            (),
            "Device invite expired successfully".to_string(),
        )),
    ))
}

/// PATCH /api/admin/devices/{id}/name - Rename a device
#[derive(Debug, Deserialize)]
pub struct RenameDeviceRequest {
    pub name: String,
}

pub async fn rename_device(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
    Json(req): Json<RenameDeviceRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let conn = &mut state.db.pool().get()?;

    // Get the old device name for audit log
    let old_device: SpaceDevice = space_devices::table
        .filter(space_devices::id.eq(id))
        .filter(space_devices::deleted_at.is_null())
        .first(conn)
        .map_err(|_| ApiError::NotFound("Device not found".to_string()))?;

    let old_name = old_device.name.clone();

    // Update the device name
    let update = UpdateSpaceDevice {
        name: Some(req.name.clone()),
        updated_at: Some(Utc::now()),
        ..Default::default()
    };

    diesel::update(space_devices::table)
        .filter(space_devices::id.eq(id))
        .set(&update)
        .execute(conn)?;

    // Create audit log
    let audit_log = NewAuditLog {
        event_type: AuditEventType::DeviceNameChanged.as_str().to_string(),
        user_id: None,
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "device_id": id,
            "old_name": old_name,
            "new_name": req.name,
        }),
        ip_address: None,
        user_agent: None,
    };

    let _ = state.db.create_audit_log(&audit_log);

    // Publish MQTT message to notify device of name change
    if let Some(mqtt_service) = &state.mqtt_service {
        if let Err(e) = mqtt_service.publish_name_change(id, &req.name) {
            tracing::warn!("Failed to publish name change to device {}: {}", id, e);
        } else {
            tracing::info!("Published name change to device {} via MQTT", id);
        }
    }

    Ok((
        StatusCode::OK,
        Json(ApiResponse::success_with_message(
            (),
            "Device renamed successfully".to_string(),
        )),
    ))
}

/// DELETE /api/admin/devices/{id} - Soft delete a device
pub async fn delete_device(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let conn = &mut state.db.pool().get()?;

    // Get device info for audit log
    let device: SpaceDevice = space_devices::table
        .filter(space_devices::id.eq(id))
        .filter(space_devices::deleted_at.is_null())
        .first(conn)
        .map_err(|_| ApiError::NotFound("Device not found".to_string()))?;

    // Soft delete the device
    diesel::update(space_devices::table)
        .filter(space_devices::id.eq(id))
        .set((
            space_devices::deleted_at.eq(Some(Utc::now())),
            space_devices::updated_at.eq(Utc::now()),
        ))
        .execute(conn)?;

    // Create audit log
    let audit_log = NewAuditLog {
        event_type: AuditEventType::DeviceDeleted.as_str().to_string(),
        user_id: None,
        actor_id: Some(admin.0.id),
        event_data: serde_json::json!({
            "device_id": id,
            "device_name": device.name,
        }),
        ip_address: None,
        user_agent: None,
    };

    let _ = state.db.create_audit_log(&audit_log);

    Ok((
        StatusCode::OK,
        Json(ApiResponse::success_with_message(
            (),
            "Device deleted successfully".to_string(),
        )),
    ))
}

// Router configuration
pub fn devices_routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register_device))
}

pub fn admin_devices_routes() -> Router<AppState> {
    Router::new()
        .route("/invite", post(create_device_invite))
        .route("/invites", get(list_device_invites))
        .route("/invites/{code}", delete(expire_device_invite))
        .route("/", get(list_devices))
        .route("/{id}/name", patch(rename_device))
        .route("/{id}", delete(delete_device))
}
