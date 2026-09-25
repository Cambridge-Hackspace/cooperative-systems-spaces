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
    /// True when the server runs metered tool billing in online-synchronous
    /// mode: the edge must ask the server before energizing this tool rather
    /// than deciding from the cached allow-list. `#[serde(default)]` so a server
    /// that does not send it (older, or edge-local mode) parses as `false`.
    #[serde(default)]
    pub requires_online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncUser {
    /// Hex `argon2id(device_pepper, card)` (#109), not the card itself.
    pub profile_field_digest: String,
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
    /// The device pepper (#109), so a swipe can be hashed the same way the
    /// server hashed the allow-list.
    ///
    /// A `CardDigester` and deliberately not a `CardCipher`: an edge has no
    /// business holding the encryption or index keys, and the type is what
    /// enforces that rather than a comment asking nicely. `None` on an edge
    /// with no pepper configured, which then authorizes nobody offline.
    card_digester: Option<Arc<css_lib::card_crypto::CardDigester>>,
}

impl ToolGuardState {
    pub fn new() -> Self {
        Self::with_digester(None)
    }

    /// The cache, with the pepper it needs to hash a swipe (#109).
    pub fn with_digester(card_digester: Option<Arc<css_lib::card_crypto::CardDigester>>) -> Self {
        let (ws_tx, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(RwLock::new(None)),
            notify_tx: None,
            ws_tx,
            card_digester,
        }
    }

    /// Create a state cache that sends a copy of each new payload to the returned receiver.
    pub fn new_with_notify(
        card_digester: Option<Arc<css_lib::card_crypto::CardDigester>>,
    ) -> (Self, std::sync::mpsc::Receiver<SyncPayload>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let (ws_tx, _) = broadcast::channel(16);
        (
            Self {
                inner: Arc::new(RwLock::new(None)),
                notify_tx: Some(tx),
                ws_tx,
                card_digester,
            },
            rx,
        )
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

    /// Check whether a card holder is authorized to activate a tool.
    ///
    /// `card_value`  – the scanned card, hashed here and matched against
    ///                 `profile_field_digest`. The cached allow-list holds
    ///                 digests rather than card identifiers (#109), so a plug
    ///                 taken off a wall yields no member's card.
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

        // No pepper means this edge cannot hash a swipe, so it cannot match
        // anything. Deny rather than fall back to comparing raw values: a
        // fallback would silently restore the behaviour this replaced, and it
        // would do so exactly when the configuration is wrong.
        let digester = match self.card_digester.as_ref() {
            Some(c) => c,
            None => return AccessResult::UnknownCard,
        };
        let presented = match digester.wire_digest(card_value) {
            Ok(d) => hex::encode(d),
            Err(_) => return AccessResult::UnknownCard,
        };

        let user = match state
            .users
            .iter()
            // #120 (#14 / L1): constant-time digest comparison, on principle --
            // the presented card digest is a credential and `==` short-circuits.
            .find(|u| css_lib::ct::constant_time_str_eq(&u.profile_field_digest, &presented))
        {
            Some(u) => u,
            None => return AccessResult::UnknownCard,
        };

        if !user.is_active {
            return AccessResult::UserInactive;
        }

        // Find the tool in the top-level tools list by external_id or UUID
        let tool = match state.tools.iter().find(|t| {
            t.external_id.as_deref() == Some(tool_id_str) || t.id.to_string() == tool_id_str
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

    /// Whether the server flagged this tool as requiring online (synchronous)
    /// actuation -- a metered tool under `actuation_mode = OnlineSynchronous`.
    /// The edge must then ask the server before energizing it rather than
    /// trusting the cached allow-list. Unknown tool / no state -> false (the
    /// offline-capable default).
    pub fn tool_requires_online(&self, tool_id_str: &str) -> bool {
        let guard = self.inner.read().unwrap();
        guard
            .as_ref()
            .and_then(|state| {
                state.tools.iter().find(|t| {
                    t.external_id.as_deref() == Some(tool_id_str) || t.id.to_string() == tool_id_str
                })
            })
            .map(|t| t.requires_online)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: Uuid, ext: &str, status: ToolStatus) -> SyncTool {
        SyncTool {
            id,
            external_id: Some(ext.to_string()),
            name: "T".to_string(),
            status,
            requires_online: false,
        }
    }
    /// The pepper these tests share. Any 32 bytes; what matters is that the
    /// fixture and the state hash with the same one, exactly as a deployment's
    /// server and edge do.
    const PEP: &str = "0404040404040404040404040404040404040404040404040404040404040404";

    fn digester() -> Arc<css_lib::card_crypto::CardDigester> {
        Arc::new(css_lib::card_crypto::CardDigester::from_hex(PEP).expect("test pepper parses"))
    }

    /// A member as the *server* would send them: the digest of their card, not
    /// the card. Building the fixture the same way the server builds the
    /// payload is what keeps these tests about matching rather than about a
    /// string this file made up.
    fn user(card: &str, active: bool, authorized: Vec<Uuid>) -> SyncUser {
        SyncUser {
            profile_field_digest: hex::encode(digester().wire_digest(card).expect("digests")),
            full_name: "N".to_string(),
            is_active: active,
            authorized_tool_ids: authorized,
        }
    }
    fn state_with(tools: Vec<SyncTool>, users: Vec<SyncUser>) -> ToolGuardState {
        let s = ToolGuardState::with_digester(Some(digester()));
        s.apply_sync(SyncPayload {
            device_id: Uuid::from_u128(0xD),
            profile_field: "card".to_string(),
            tools,
            users,
        });
        s
    }

    /// An edge with no pepper cannot hash a swipe, so it must refuse rather
    /// than compare raw values. Falling back would silently restore exactly the
    /// behaviour #109 removed, and it would do so precisely when somebody had
    /// misconfigured the device -- the worst moment to be lenient.
    #[test]
    fn an_edge_without_a_pepper_authorizes_nobody() {
        let tid = Uuid::from_u128(1);
        let s = ToolGuardState::with_digester(None);
        s.apply_sync(SyncPayload {
            device_id: Uuid::from_u128(0xD),
            profile_field: "card".to_string(),
            tools: vec![tool(tid, "t-ext", ToolStatus::Idle)],
            users: vec![user("card-1", true, vec![tid])],
        });
        assert_eq!(
            AccessResult::UnknownCard,
            s.check_access("card-1", "t-ext"),
            "without a pepper an edge must deny, not compare the raw card"
        );
    }

    /// And the digest must not be mistaken for the card. A payload carrying
    /// digests must not authorize somebody who presents the digest itself --
    /// which is what a stolen device's cache would hand an attacker.
    #[test]
    fn presenting_the_digest_instead_of_the_card_is_refused() {
        let tid = Uuid::from_u128(1);
        let s = state_with(
            vec![tool(tid, "t-ext", ToolStatus::Idle)],
            vec![user("card-1", true, vec![tid])],
        );
        let digest = hex::encode(digester().wire_digest("card-1").expect("digests"));
        assert_eq!(
            AccessResult::Authorized,
            s.check_access("card-1", "t-ext"),
            "precondition: the real card works"
        );
        assert_eq!(
            AccessResult::UnknownCard,
            s.check_access(&digest, "t-ext"),
            "the digest is not a credential; replaying it must not open the tool"
        );
    }

    #[test]
    fn cold_start_denies_to_be_safe() {
        // No sync payload loaded yet: deny rather than energize. This is the
        // fail-secure property, and it is the negative control that keeps the
        // "authorized" case below from being vacuous.
        let s = ToolGuardState::with_digester(Some(digester()));
        assert_eq!(s.check_access("card-1", "t-ext"), AccessResult::UnknownCard);
    }

    #[test]
    fn authorized_when_active_listed_and_idle() {
        let tid = Uuid::from_u128(1);
        let s = state_with(
            vec![tool(tid, "t-ext", ToolStatus::Idle)],
            vec![user("card-1", true, vec![tid])],
        );
        assert_eq!(s.check_access("card-1", "t-ext"), AccessResult::Authorized);
        // The tool id resolves by external id OR the UUID string.
        assert_eq!(
            s.check_access("card-1", &tid.to_string()),
            AccessResult::Authorized
        );
    }

    #[test]
    fn unknown_card_is_denied() {
        let tid = Uuid::from_u128(1);
        let s = state_with(
            vec![tool(tid, "t-ext", ToolStatus::Idle)],
            vec![user("card-1", true, vec![tid])],
        );
        assert_eq!(s.check_access("nope", "t-ext"), AccessResult::UnknownCard);
    }

    #[test]
    fn inactive_user_is_denied_before_the_tool_is_even_looked_up() {
        let tid = Uuid::from_u128(1);
        // The user is otherwise fully authorized -- only is_active flips -- so a
        // pass here would mean the active gate did nothing.
        let s = state_with(
            vec![tool(tid, "t-ext", ToolStatus::Idle)],
            vec![user("card-1", false, vec![tid])],
        );
        assert_eq!(
            s.check_access("card-1", "t-ext"),
            AccessResult::UserInactive
        );
    }

    #[test]
    fn a_tool_absent_from_the_payload_is_not_authorized() {
        let tid = Uuid::from_u128(1);
        let s = state_with(
            vec![tool(tid, "t-ext", ToolStatus::Idle)],
            vec![user("card-1", true, vec![tid])],
        );
        assert_eq!(
            s.check_access("card-1", "some-other-tool"),
            AccessResult::ToolNotAuthorized
        );
    }

    #[test]
    fn a_tool_the_user_is_not_listed_for_is_not_authorized() {
        let tid = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        // Tool exists and user is active, but the user's allow-list holds a
        // DIFFERENT tool -- isolates the per-tool authorization check.
        let s = state_with(
            vec![tool(tid, "t-ext", ToolStatus::Idle)],
            vec![user("card-1", true, vec![other])],
        );
        assert_eq!(
            s.check_access("card-1", "t-ext"),
            AccessResult::ToolNotAuthorized
        );
    }

    #[test]
    fn a_non_idle_tool_is_unavailable_and_names_its_status() {
        let tid = Uuid::from_u128(1);
        // Self-test the Idle gate: an otherwise-authorized scan is refused for
        // every non-Idle status, and the status word is surfaced lowercased.
        for (st, word) in [
            (ToolStatus::InUse, "inuse"),
            (ToolStatus::Maintenance, "maintenance"),
            (ToolStatus::Broken, "broken"),
            (ToolStatus::Repair, "repair"),
            (ToolStatus::Retired, "retired"),
        ] {
            let s = state_with(
                vec![tool(tid, "t-ext", st.clone())],
                vec![user("card-1", true, vec![tid])],
            );
            assert_eq!(
                s.check_access("card-1", "t-ext"),
                AccessResult::ToolUnavailable(word.to_string()),
                "status {:?} should be reported unavailable and lowercased",
                st
            );
        }
    }

    #[test]
    fn requires_online_reflects_the_flag_and_defaults_offline_capable() {
        let tid = Uuid::from_u128(1);
        let mut flagged = tool(tid, "t-ext", ToolStatus::Idle);
        flagged.requires_online = true;
        let s = state_with(vec![flagged], vec![]);
        assert!(s.tool_requires_online("t-ext"), "matched by external id");
        assert!(s.tool_requires_online(&tid.to_string()), "matched by uuid");
        assert!(
            !s.tool_requires_online("unknown-tool"),
            "an unknown tool defaults to offline-capable"
        );

        // A tool present but not flagged -> false.
        let s2 = state_with(vec![tool(tid, "t-ext", ToolStatus::Idle)], vec![]);
        assert!(!s2.tool_requires_online("t-ext"));

        // No state at all -> false (the offline-capable default).
        assert!(!ToolGuardState::new().tool_requires_online("t-ext"));
    }
}
