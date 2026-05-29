//! Door access service: compiles per-device state snapshots from the
//! configured access rules, publishes those snapshots over MQTT so the edge
//! can decide locally, and supports server-initiated unlocks (QR check-in
//! and admin remote unlock).
//!
//! Rule evaluation: `deny` always beats `allow`. A user passes when any
//! `allow` rule matches *and* no `deny` rule matches. `kind=role` means
//! "this role or higher" using the standard hierarchy
//! Newbie < Member < Staff < Admin.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::ConfigManager;
use crate::database::{DatabaseError, DatabaseManager};
use crate::devices_transport::DeviceTransport;
use crate::models::{
    Door, DoorAccessRule, DoorRuleEffect, DoorRuleKind, Schedule, User, UserRole,
};

#[derive(Debug, Clone, Serialize)]
pub struct CompiledDoor {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub unlock_duration_ms: i32,
    pub allow_cards: Vec<String>,
    pub deny_cards: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoorStateSnapshot {
    pub snapshot_at: chrono::DateTime<Utc>,
    pub doors: Vec<CompiledDoor>,
}

/// Outcome of evaluating access for a known user (used by the QR flow).
#[derive(Debug, Clone)]
pub enum AccessDecision {
    Allow,
    Deny(String),
}

#[derive(Clone)]
pub struct DoorService {
    db: Arc<DatabaseManager>,
    transport: Arc<DeviceTransport>,
    profile_field: String,
    config: Arc<ConfigManager>,
    /// Last-published snapshot hash per device. Used by the schedule ticker
    /// to skip republish when nothing changed.
    last_snapshot_hash: Arc<std::sync::Mutex<std::collections::HashMap<Uuid, u64>>>,
}

impl DoorService {
    pub fn new(
        db: Arc<DatabaseManager>,
        transport: Arc<DeviceTransport>,
        profile_field: String,
        config: Arc<ConfigManager>,
    ) -> Self {
        Self {
            db,
            transport,
            profile_field,
            config,
            last_snapshot_hash: Arc::new(std::sync::Mutex::new(Default::default())),
        }
    }

    // ----- card extraction helpers -------------------------------------

    /// Pull all card values out of a user's profile JSONB at the configured
    /// field. Accepts either a scalar string or an array of strings (matches
    /// what the new TextArray profile-field shape stores).
    fn cards_for_user(&self, user: &User) -> Vec<String> {
        match user.profile.get(&self.profile_field) {
            Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    // ----- state compilation -------------------------------------------

    /// Build the snapshot for one device by walking each of its doors'
    /// access rules and expanding them into flat card lists. Role rules are
    /// expanded by walking the active-user roster once.
    pub fn compile_state_for(
        &self,
        device_id: Uuid,
    ) -> Result<DoorStateSnapshot, DatabaseError> {
        let doors = self.db.list_doors_for_device(device_id)?;
        if doors.is_empty() {
            return Ok(DoorStateSnapshot {
                snapshot_at: Utc::now(),
                doors: Vec::new(),
            });
        }

        // Single pass over active users for role/user rule expansion.
        let active_users = self.db.list_active_users()?;
        // Snapshot the schedules table so rule expansion is a pure CPU pass.
        let schedules = self.db.list_schedules()?;
        let tz = self.site_tz();

        let mut compiled = Vec::with_capacity(doors.len());
        for door in &doors {
            let rules = self.db.list_rules_for_door(door.id)?;
            let (allow, deny) = self.expand_rules(&rules, &active_users, &schedules, tz);
            compiled.push(CompiledDoor {
                id: door.id,
                name: door.name.clone(),
                enabled: door.enabled,
                unlock_duration_ms: door.unlock_duration_ms,
                allow_cards: allow.into_iter().collect(),
                deny_cards: deny.into_iter().collect(),
            });
        }

        Ok(DoorStateSnapshot {
            snapshot_at: Utc::now(),
            doors: compiled,
        })
    }

    fn expand_rules(
        &self,
        rules: &[DoorAccessRule],
        active_users: &[User],
        schedules: &[Schedule],
        tz: chrono_tz::Tz,
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let mut allow = BTreeSet::<String>::new();
        let mut deny = BTreeSet::<String>::new();

        for rule in rules {
            // Rule's schedule, if any, must be in-window right now;
            // otherwise the rule contributes nothing to either bucket.
            if !schedule_is_active_now(rule.schedule_id, schedules, tz) {
                continue;
            }
            let effect = DoorRuleEffect::parse(&rule.effect).unwrap_or(DoorRuleEffect::Allow);
            let kind = match DoorRuleKind::parse(&rule.kind) {
                Some(k) => k,
                None => {
                    warn!("Skipping door rule with unknown kind '{}'", rule.kind);
                    continue;
                }
            };
            let bucket = match effect {
                DoorRuleEffect::Allow => &mut allow,
                DoorRuleEffect::Deny => &mut deny,
            };
            match kind {
                DoorRuleKind::Card => {
                    bucket.insert(rule.value.clone());
                }
                DoorRuleKind::User => {
                    if let Ok(uid) = Uuid::parse_str(&rule.value) {
                        if let Some(u) = active_users.iter().find(|u| u.id == uid) {
                            for c in self.cards_for_user(u) {
                                bucket.insert(c);
                            }
                        }
                    } else {
                        warn!("Door rule of kind=user has non-UUID value '{}'", rule.value);
                    }
                }
                DoorRuleKind::Role => {
                    let required = match role_from_str(&rule.value) {
                        Some(r) => r,
                        None => {
                            warn!("Door rule of kind=role has unknown role '{}'", rule.value);
                            continue;
                        }
                    };
                    for u in active_users.iter() {
                        if role_level(&u.role) >= role_level(&required) {
                            for c in self.cards_for_user(u) {
                                bucket.insert(c);
                            }
                        }
                    }
                }
            }
        }

        (allow, deny)
    }

    // ----- publishing ---------------------------------------------------

    /// Publish a fresh snapshot for one device. The transport picks WS if
    /// the device is connected via WebSocket, otherwise MQTT, otherwise warns.
    pub fn publish_state(&self, device_id: Uuid) -> Result<(), DatabaseError> {
        let snapshot = self.compile_state_for(device_id)?;
        let payload = serde_json::to_value(&snapshot)
            .map_err(|e| DatabaseError::Other(format!("serialize doors snapshot: {e}")))?;
        let count = snapshot.doors.len();
        if self
            .transport
            .push(device_id, css_lib::wire::kinds::DOORS_STATE, payload)
        {
            info!("Published doors/state to {} ({} door(s))", device_id, count);
        } else {
            debug!("Skipped doors/state to {} (no transport)", device_id);
        }
        Ok(())
    }

    /// Republish state to every edge device that has at least one door.
    /// Called after role/activation/profile-card changes that don't tell us
    /// exactly which devices are affected.
    pub fn republish_all(&self) {
        let device_ids = match self.db.list_door_device_ids() {
            Ok(ids) => ids,
            Err(e) => {
                error!("Failed to list door device IDs for republish: {}", e);
                return;
            }
        };
        for id in device_ids {
            if let Err(e) = self.publish_state(id) {
                error!("Republish to {} failed: {}", id, e);
            }
        }
    }

    /// One-shot unlock command — used by QR check-in and admin remote unlock.
    pub fn publish_unlock(
        &self,
        device_id: Uuid,
        door_id: Uuid,
        duration_ms: i32,
        reason: &str,
    ) -> Result<(), DatabaseError> {
        let payload = serde_json::json!({
            "door_id": door_id,
            "duration_ms": duration_ms,
            "reason": reason,
        });
        if !self
            .transport
            .push(device_id, css_lib::wire::kinds::DOORS_UNLOCK, payload)
        {
            return Err(DatabaseError::Other(
                "No transport available to deliver doors/unlock".into(),
            ));
        }
        Ok(())
    }

    // ----- evaluation (QR check-in path) -------------------------------

    /// Decide whether `user` can unlock `door` right now. Used by the
    /// server-side QR check-in handler. Mirrors the same allow/deny logic
    /// applied at edge for RFID scans, but operates on the user rather than
    /// a raw card ID so we can match `kind=user` rules even when the user
    /// has no card on file.
    pub fn evaluate(&self, door: &Door, user: &User) -> Result<AccessDecision, DatabaseError> {
        if !door.enabled {
            return Ok(AccessDecision::Deny("Door is disabled".into()));
        }
        if !user.is_active {
            return Ok(AccessDecision::Deny("Account inactive".into()));
        }
        let rules = self.db.list_rules_for_door(door.id)?;
        let schedules = self.db.list_schedules()?;
        let tz = self.site_tz();
        let mut allow = HashSet::<String>::new();
        let mut deny = HashSet::<String>::new();
        let user_cards: HashSet<String> = self.cards_for_user(user).into_iter().collect();
        let user_id_str = user.id.to_string();

        for rule in &rules {
            // Schedule-gated rules are silent when their window is closed.
            if !schedule_is_active_now(rule.schedule_id, &schedules, tz) {
                continue;
            }
            let effect = DoorRuleEffect::parse(&rule.effect).unwrap_or(DoorRuleEffect::Allow);
            let kind = match DoorRuleKind::parse(&rule.kind) {
                Some(k) => k,
                None => continue,
            };
            let matched = match kind {
                DoorRuleKind::Card => user_cards.contains(&rule.value),
                DoorRuleKind::User => rule.value == user_id_str,
                DoorRuleKind::Role => match role_from_str(&rule.value) {
                    Some(required) => role_level(&user.role) >= role_level(&required),
                    None => false,
                },
            };
            if !matched {
                continue;
            }
            match effect {
                DoorRuleEffect::Allow => {
                    allow.insert(rule.id.to_string());
                }
                DoorRuleEffect::Deny => {
                    deny.insert(rule.id.to_string());
                }
            }
        }

        if !deny.is_empty() {
            return Ok(AccessDecision::Deny("Denied by access rule".into()));
        }
        if !allow.is_empty() {
            Ok(AccessDecision::Allow)
        } else {
            Ok(AccessDecision::Deny("No matching access rule".into()))
        }
    }

    // ----- schedules helpers + ticker ----------------------------------

    fn site_tz(&self) -> chrono_tz::Tz {
        let cfg = self.config.get_config();
        crate::schedules::resolve_tz(&cfg.site.timezone)
    }

    /// Recompile per-device state and only push when the snapshot's hash
    /// has changed since the last successful publish. Quiet during steady
    /// state — the only thing chatty is a real schedule transition.
    pub fn republish_changed(&self) {
        let device_ids = match self.db.list_door_device_ids() {
            Ok(ids) => ids,
            Err(e) => {
                error!("Failed to list door device IDs for tick republish: {}", e);
                return;
            }
        };
        for id in device_ids {
            match self.compile_state_for(id) {
                Ok(snapshot) => {
                    let bytes = match serde_json::to_vec(&snapshot) {
                        Ok(b) => b,
                        Err(e) => {
                            warn!("Failed to serialize snapshot for {}: {}", id, e);
                            continue;
                        }
                    };
                    let hash = stable_hash(&bytes);
                    let mut map = self
                        .last_snapshot_hash
                        .lock()
                        .expect("door snapshot hash map poisoned");
                    if map.get(&id).copied() == Some(hash) {
                        continue;
                    }
                    let payload = match serde_json::to_value(&snapshot) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Failed to encode snapshot value for {}: {}", id, e);
                            continue;
                        }
                    };
                    drop(map);
                    if self
                        .transport
                        .push(id, css_lib::wire::kinds::DOORS_STATE, payload)
                    {
                        info!(
                            "Schedule tick republished doors/state to {} ({} door(s))",
                            id, snapshot.doors.len()
                        );
                        let mut map = self
                            .last_snapshot_hash
                            .lock()
                            .expect("door snapshot hash map poisoned");
                        map.insert(id, hash);
                    }
                }
                Err(e) => warn!("Failed to compile state for {}: {}", id, e),
            }
        }
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Resolve a rule's schedule (by id) and ask whether *now* falls in any
/// interval. Rules with no schedule are always active.
fn schedule_is_active_now(
    schedule_id: Option<Uuid>,
    schedules: &[Schedule],
    tz: chrono_tz::Tz,
) -> bool {
    let sid = match schedule_id {
        Some(id) => id,
        None => return true,
    };
    let sched = match schedules.iter().find(|s| s.id == sid) {
        Some(s) => s,
        None => {
            // Schedule went missing (deleted mid-compile). Treat as always
            // — matches the FK ON DELETE SET NULL semantics on the next
            // recompile.
            return true;
        }
    };
    let intervals = match crate::schedules::parse_intervals(&sched.intervals) {
        Ok(v) => v,
        Err(e) => {
            warn!("Schedule {} has invalid intervals: {}", sched.id, e);
            return false;
        }
    };
    crate::schedules::matches_now(&intervals, tz)
}

fn role_level(role: &UserRole) -> u8 {
    match role {
        UserRole::Unknown => 0,
        UserRole::Newbie => 1,
        UserRole::Member => 2,
        UserRole::Staff => 3,
        UserRole::Admin => 4,
    }
}

fn role_from_str(s: &str) -> Option<UserRole> {
    match s.to_ascii_lowercase().as_str() {
        "unknown" => Some(UserRole::Unknown),
        "newbie" => Some(UserRole::Newbie),
        "member" => Some(UserRole::Member),
        "staff" => Some(UserRole::Staff),
        "admin" => Some(UserRole::Admin),
        _ => None,
    }
}
