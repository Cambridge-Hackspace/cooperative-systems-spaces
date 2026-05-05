use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use uuid::Uuid;

// ── Tool status enum (mirrors server models::ToolStatus) ─────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Idle,
    InUse,
    Maintenance,
    Broken,
    Repair,
    Retired,
}

// ── Sync payload (mirrors server's ToolGuardSyncPayload) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTool {
    pub id: Uuid,
    pub external_id: Option<String>,
    pub name: String,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncUser {
    pub profile_field_value: String,
    pub full_name: String,
    pub is_active: bool,
    pub authorized_tool_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub device_id: Uuid,
    pub profile_field: String,
    pub tools: Vec<SyncTool>,
    pub users: Vec<SyncUser>,
}

// ── Access check result ───────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum AccessResult {
    Authorized,
    UnknownCard,
    UserInactive,
    ToolNotAuthorized,
    ToolUnavailable(String),
}

// ── State cache ───────────────────────────────────────────────────────────────

pub struct ToolGuardState {
    inner: Arc<RwLock<Option<SyncPayload>>>,
    /// Fired on every state change so consumers (e.g. local MQTT publisher) can react.
    notify_tx: Option<std::sync::mpsc::SyncSender<SyncPayload>>,
    /// Broadcast channel for WebSocket clients — sends serialized JSON on every state change.
    ws_tx: broadcast::Sender<String>,
}

impl ToolGuardState {
    pub fn new() -> Self {
        let (ws_tx, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(RwLock::new(None)),
            notify_tx: None,
            ws_tx,
        }
    }

    /// Create a state cache that sends a copy of each new payload to the returned receiver.
    pub fn new_with_notify() -> (Self, std::sync::mpsc::Receiver<SyncPayload>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let (ws_tx, _) = broadcast::channel(16);
        (Self {
            inner: Arc::new(RwLock::new(None)),
            notify_tx: Some(tx),
            ws_tx,
        }, rx)
    }

    /// Subscribe to WebSocket state-change notifications (serialized JSON).
    pub fn subscribe_ws(&self) -> broadcast::Receiver<String> {
        self.ws_tx.subscribe()
    }

    /// Replace the cached state with a freshly received sync payload.
    pub fn apply_sync(&self, payload: SyncPayload) {
        let mut guard = self.inner.write().unwrap();
        *guard = Some(payload.clone());
        drop(guard);
        if let Some(tx) = &self.notify_tx {
            let _ = tx.try_send(payload.clone());
        }
        if let Ok(json) = serde_json::to_string(&payload) {
            let _ = self.ws_tx.send(json);
        }
    }

    /// Apply a sync payload received as raw JSON bytes (e.g. from MQTT).
    pub fn apply_sync_bytes(&self, bytes: &[u8]) -> Result<(), serde_json::Error> {
        let payload: SyncPayload = serde_json::from_slice(bytes)?;
        self.apply_sync(payload);
        Ok(())
    }

    /// Whether any state has been loaded yet.
    pub fn has_state(&self) -> bool {
        self.inner.read().unwrap().is_some()
    }

    /// Return a clone of the current state, if any.
    pub fn get_state(&self) -> Option<SyncPayload> {
        self.inner.read().unwrap().clone()
    }

    /// Check whether a card holder is authorised to activate a tool.
    ///
    /// `card_value`  – the scanned card value (matched against `profile_field_value`)
    /// `tool_id_str` – the hardware tool identifier (matched against `external_id` first,
    ///                 then the UUID `id` as a string fallback)
    pub fn check_access(&self, card_value: &str, tool_id_str: &str) -> AccessResult {
        let guard = self.inner.read().unwrap();
        let state = match guard.as_ref() {
            Some(s) => s,
            None => {
                // No state loaded yet — deny to be safe
                return AccessResult::UnknownCard;
            }
        };

        let user = match state.users.iter().find(|u| u.profile_field_value == card_value) {
            Some(u) => u,
            None => return AccessResult::UnknownCard,
        };

        if !user.is_active {
            return AccessResult::UserInactive;
        }

        // Find the tool in the top-level tools list by external_id or UUID
        let tool = match state.tools.iter().find(|t| {
            t.external_id.as_deref() == Some(tool_id_str)
                || t.id.to_string() == tool_id_str
        }) {
            Some(t) => t,
            None => return AccessResult::ToolNotAuthorized,
        };

        // Check the user is authorized for this specific tool
        if !user.authorized_tool_ids.contains(&tool.id) {
            return AccessResult::ToolNotAuthorized;
        }

        if tool.status != ToolStatus::Idle {
            return AccessResult::ToolUnavailable(format!("{:?}", tool.status).to_lowercase());
        }

        AccessResult::Authorized
    }
}
