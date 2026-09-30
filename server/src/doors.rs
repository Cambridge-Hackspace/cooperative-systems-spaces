//! Door access service: compiles per-device state snapshots from the
//! configured access rules, publishes those snapshots over MQTT so the edge
//! can decide locally, and supports server-initiated unlocks (QR check-in
//! and admin remote unlock).
//!
//! Rule evaluation: `deny` always beats `allow`. A user passes when any
//! `allow` rule matches *and* no `deny` rule matches. `kind=role` means
//! "this role or higher" using the standard hierarchy
//! Newbie < Member < Staff < Admin.

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::ConfigManager;
use crate::database::{DatabaseError, DatabaseManager};
use crate::devices_transport::DeviceTransport;
use crate::models::{AccessRule, Door, DoorRuleEffect, DoorRuleKind, Schedule, User};

#[derive(Debug, Clone, Serialize)]
pub struct CompiledDoor {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub unlock_duration_ms: i32,
    pub allow_cards: Vec<String>,
    pub deny_cards: Vec<String>,
    /// When set, the door's strike is held unlocked (no card required) until
    /// this instant -- the Open Access latch (issue #12). `None` = normal
    /// card-gated behavior. The edge compares this to its own clock, so a
    /// held window self-expires even if the closing push never arrives
    /// (fail-secure + edge-local expiry).
    pub hold_unlock_until: Option<DateTime<Utc>>,
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
    /// #120 (#122/H3): digests door card tokens before they leave the server, so
    /// a stolen edge controller's snapshot yields no plaintext card codes.
    card_cipher: Option<Arc<css_lib::card_crypto::CardCipher>>,
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
        card_cipher: Option<Arc<css_lib::card_crypto::CardCipher>>,
    ) -> Self {
        Self {
            db,
            transport,
            profile_field,
            config,
            card_cipher,
            last_snapshot_hash: Arc::new(std::sync::Mutex::new(Default::default())),
        }
    }

    // ----- state compilation -------------------------------------------

    /// Build the snapshot for one device by walking each of its doors'
    /// access rules and expanding them into flat card lists. Role rules are
    /// expanded by walking the active-user roster once.
    pub fn compile_state_for(&self, device_id: Uuid) -> Result<DoorStateSnapshot, DatabaseError> {
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

        // Effective tier level per active user, computed once for role-rule
        // expansion (replaces the per-user rank() read on the dropped column).
        let user_levels = self
            .db
            .effective_levels_for(&active_users.iter().map(|u| u.id).collect::<Vec<_>>())?;

        // #120 (#122/H3+H4): per-user card wire-digests (first-class cards unioned
        // with digested legacy profile values), computed once. The snapshot ships
        // these digests, never plaintext codes.
        let cards_by_user = self.db.card_digests_by_user(
            &active_users,
            &self.profile_field,
            self.card_cipher.as_deref(),
        )?;

        let mut compiled = Vec::with_capacity(doors.len());
        for door in &doors {
            let rules = self.db.list_rules_for_door(door.id)?;
            let (allow, deny) =
                self.expand_rules(&rules, &cards_by_user, &schedules, tz, &user_levels);
            let hold_unlock_until = open_access_hold_until_at(&rules, &schedules, tz, Utc::now());
            compiled.push(CompiledDoor {
                id: door.id,
                name: door.name.clone(),
                enabled: door.enabled,
                unlock_duration_ms: door.unlock_duration_ms,
                allow_cards: allow.into_iter().collect(),
                deny_cards: deny.into_iter().collect(),
                hold_unlock_until,
            });
        }

        Ok(DoorStateSnapshot {
            snapshot_at: Utc::now(),
            doors: compiled,
        })
    }

    fn expand_rules(
        &self,
        rules: &[AccessRule],
        cards_by_user: &std::collections::HashMap<Uuid, Vec<String>>,
        schedules: &[Schedule],
        tz: chrono_tz::Tz,
        user_levels: &std::collections::HashMap<Uuid, i16>,
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let graph = self.db.rbac();
        expand_rules_at(
            rules,
            cards_by_user,
            schedules,
            tz,
            Utc::now(),
            &graph,
            user_levels,
        )
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
        let mut payload = serde_json::json!({
            "door_id": door_id,
            "duration_ms": duration_ms,
            "reason": reason,
        });
        // #120 (#121): HMAC-sign the command with the device's key so the edge
        // rejects any doors/unlock not signed by this server. Only when the
        // device has a key (registered since #121); otherwise it goes unsigned
        // and the edge, also keyless, accepts it -- graceful during rollout, with
        // the broker per-device ACLs as the primary control throughout.
        if let Some(cipher) = self.card_cipher.as_deref() {
            if let Some(key) = self.db.device_command_key(device_id, cipher)? {
                let msg =
                    css_lib::sig::doors_unlock_message(&door_id.to_string(), duration_ms, reason);
                payload["sig"] = serde_json::Value::String(css_lib::sig::sign(&key, &msg));
            }
        }
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
    /// `action` distinguishes an in-person check-in from a remote unlock. Both run
    /// the same rules -- #101 decided that a remote unlock *composes with* a
    /// resource's rules rather than bypassing them -- so this takes the action
    /// rather than assuming one, and there is still exactly one decision path.
    pub fn evaluate(
        &self,
        door: &Door,
        user: &User,
        action: crate::access_engine::Action,
    ) -> Result<AccessDecision, DatabaseError> {
        // #101: the QR check-in decision now runs through the one access engine
        // (`access_engine::may`), the same engine the tool path resolves through,
        // so the two cannot diverge. This method materializes the door's policy and
        // this user's principal and delegates; the rule-matching semantics are
        // unchanged (they moved into `may`, which reuses the same
        // `schedule_state_at`/`rule_fires`/`DoorRuleKind` primitives), and the deny
        // messages are preserved verbatim below. Pinned by contracts/door_rules.json
        // (`may_reproduces_the_per_principal_door_decision`).
        use crate::access_engine::{
            may, Decision, DefaultEffect, DenyReason, Principal, ResourcePolicy,
        };

        let rules = self.db.list_rules_for_door(door.id)?;
        let schedules = self.db.list_schedules()?;
        let tz = self.site_tz();
        // #120 (#122/H3+H4): a kind=card rule stores a card wire-digest at rest, so
        // match it against THIS user's card digests -- first-class cards unioned
        // with digested legacy profile values, exactly as the compiled snapshot does.
        let card_digests: BTreeSet<String> = self
            .db
            .card_digests_by_user(
                std::slice::from_ref(user),
                &self.profile_field,
                self.card_cipher.as_deref(),
            )?
            .remove(&user.id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let graph = self.db.rbac();

        let principal = Principal {
            user_id: Some(user.id),
            level: self.db.user_effective_level(user.id)?,
            card_digests,
            is_active: user.is_active,
        };
        let policy = ResourcePolicy {
            // Doors carry no lockout today; the disabled flag is availability.
            locked_out: false,
            unavailable: (!door.enabled).then(|| "Door is disabled".to_string()),
            // A door is restricted: no matching allow rule -> denied.
            default_effect: DefaultEffect::Restricted,
            // Door schedules live on the rules, not the resource.
            schedule_id: None,
            training_ok: None,
            metered_ok: None,
        };

        let decision = may(
            &principal,
            &policy,
            &rules,
            &schedules,
            tz,
            Utc::now(),
            graph.as_ref(),
            action,
        );
        Ok(match decision {
            Decision::Allow => AccessDecision::Allow,
            // Messages preserved verbatim from the previous inline evaluator.
            Decision::Deny(DenyReason::Unavailable(m)) => AccessDecision::Deny(m),
            Decision::Deny(DenyReason::Inactive) => AccessDecision::Deny("Account inactive".into()),
            Decision::Deny(DenyReason::Rule) => {
                AccessDecision::Deny("Denied by access rule".into())
            }
            Decision::Deny(DenyReason::NoMatch) => {
                AccessDecision::Deny("No matching access rule".into())
            }
            // A door cannot produce these today (no lockout, no training/metering),
            // but map them rather than panic if the policy ever carries them.
            Decision::Deny(DenyReason::LockedOut) => AccessDecision::Deny("Locked out".into()),
            Decision::Deny(DenyReason::Training | DenyReason::Billing) => {
                AccessDecision::Deny("Denied".into())
            }
        })
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
                    // Read-only: the write happens after the publish, under a
                    // second lock below, after `drop(map)`, so that a failed publish
                    // does not record the snapshot as sent.
                    let map = self
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
                            id,
                            snapshot.doors.len()
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
/// Whether a rule's schedule window is open, shut, or cannot be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduleState {
    /// `now` falls inside the window (or the rule has no schedule).
    Active,
    /// `now` falls outside a well-defined window.
    Inactive,
    /// The schedule row is missing (a deleted schedule, whose FK is
    /// `ON DELETE SET NULL`, or the narrow read-time race between the
    /// `list_rules_for_door` and `list_schedules` snapshots) or its intervals do
    /// not parse. #120 (#122/#7): this ambiguity must *tighten* access, never
    /// widen it -- an allow rule is dropped, a deny rule still applies.
    Unresolvable,
}

pub(crate) fn schedule_state_at(
    schedule_id: Option<Uuid>,
    schedules: &[Schedule],
    tz: chrono_tz::Tz,
    now: chrono::DateTime<Utc>,
) -> ScheduleState {
    let sid = match schedule_id {
        Some(id) => id,
        None => return ScheduleState::Active,
    };
    let sched = match schedules.iter().find(|s| s.id == sid) {
        Some(s) => s,
        None => {
            warn!(
                "Schedule {} referenced but not found in current snapshot",
                sid
            );
            return ScheduleState::Unresolvable;
        }
    };
    let intervals = match crate::schedules::parse_intervals(&sched.intervals) {
        Ok(v) => v,
        Err(e) => {
            warn!("Schedule {} has invalid intervals: {}", sched.id, e);
            return ScheduleState::Unresolvable;
        }
    };
    if crate::schedules::matches_at(&intervals, tz, now) {
        ScheduleState::Active
    } else {
        ScheduleState::Inactive
    }
}

/// Whether a rule of `effect` fires now, given its schedule's state. Fail-closed:
/// an unresolvable schedule keeps a deny in force but drops an allow (#122/#7).
pub(crate) fn rule_fires(effect: DoorRuleEffect, state: ScheduleState) -> bool {
    match state {
        ScheduleState::Active => true,
        ScheduleState::Inactive => false,
        ScheduleState::Unresolvable => matches!(effect, DoorRuleEffect::Deny),
    }
}

// ---------------------------------------------------------------------------
// Pure rule expansion
// ---------------------------------------------------------------------------
//
// Promoted out of `DoorService` so they can be driven by the shared vector file
// in `contracts/door_rules.json`. Neither ever touched `self.db` -- the only
// thing they needed from `self` was `profile_field` -- so this is a move, not a
// rewrite, and the methods above are now one-line wrappers over these.
//
// Three implementations have to agree about door access: this compilation, the
// QR path in `DoorService::evaluate`, and `DoorsState::decide` on the edge.
// None of them is the oracle; the vector file is.

/// Every card value stored at `field` in a user's profile JSONB.
///
/// Accepts a scalar string or an array of strings, matching what the TextArray
/// profile-field shape stores.
pub fn cards_in_profile(profile: &Value, field: &str) -> Vec<String> {
    match profile.get(field) {
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

/// The instant an Open Access door's held-unlock window ends, or `None` if no
/// open-access rule is holding it open at `now`.
///
/// Only `allow` open-access rules count — a `deny` open-access rule is inert
/// (the retained Effect field is a UI affordance, not a force-lock). Across
/// several simultaneously-active windows the latest end wins, so the door stays
/// open until the last one closes. An open-access rule with no schedule is
/// skipped: there is no window end, and the add-rule handler already refuses to
/// create one (an unscheduled latch would hold the door open forever).
///
/// This is the door-level counterpart to `expand_rules_at`; both are pure so the
/// `contracts/door_rules.json` vectors can drive them without a database.
pub fn open_access_hold_until_at(
    rules: &[AccessRule],
    schedules: &[Schedule],
    tz: chrono_tz::Tz,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let mut latest: Option<DateTime<Utc>> = None;
    for rule in rules {
        if DoorRuleKind::parse(&rule.kind) != Some(DoorRuleKind::OpenAccess) {
            continue;
        }
        if DoorRuleEffect::parse(&rule.effect) != Some(DoorRuleEffect::Allow) {
            continue; // deny is inert for open access
        }
        let sid = match rule.schedule_id {
            Some(id) => id,
            None => continue,
        };
        let sched = match schedules.iter().find(|s| s.id == sid) {
            Some(s) => s,
            None => continue, // read-time race: schedule gone from this snapshot
        };
        let intervals = match crate::schedules::parse_intervals(&sched.intervals) {
            Ok(v) => v,
            Err(e) => {
                warn!("Open-access rule {} has invalid intervals: {}", rule.id, e);
                continue;
            }
        };
        if let Some(end) = crate::schedules::active_until(&intervals, tz, now) {
            latest = Some(latest.map_or(end, |cur| cur.max(end)));
        }
    }
    latest
}

/// Expand access rules into flat allow/deny card-token sets, as of `now`.
///
/// #120 (#122/H3+H4): the tokens are card *wire-digests*, not plaintext codes.
/// `cards_by_user` maps a user to the digests that identify them (first-class
/// cards unioned with digested legacy profile values -- see
/// `DatabaseManager::card_digests_by_user`), so a `kind=user`/`kind=role` rule
/// expands to those digests and a `kind=card` rule contributes `rule.value`
/// (stored as a digest at rest). This function stays pure and free of the
/// cipher: it routes opaque tokens, which is exactly what the golden vectors in
/// `contracts/door_rules.json` pin.
pub fn expand_rules_at(
    rules: &[AccessRule],
    cards_by_user: &std::collections::HashMap<Uuid, Vec<String>>,
    schedules: &[Schedule],
    tz: chrono_tz::Tz,
    now: chrono::DateTime<Utc>,
    graph: &crate::rbac::RoleGraph,
    user_levels: &std::collections::HashMap<Uuid, i16>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut allow = BTreeSet::<String>::new();
    let mut deny = BTreeSet::<String>::new();

    for rule in rules {
        // An unparseable effect used to default to Allow. On a door, that is a
        // fail-open: a typo in a rule's effect column silently granted access
        // to whatever the rule named. It is skipped now, and loudly. Parsed
        // before the schedule gate because the gate is fail-closed per effect.
        let effect = match DoorRuleEffect::parse(&rule.effect) {
            Some(e) => e,
            None => {
                warn!(
                    "Skipping door rule {} with unrecognized effect '{}' -- refusing to \
                     guess; an unknown effect used to be treated as allow",
                    rule.id, rule.effect
                );
                continue;
            }
        };

        // A schedule-gated rule contributes nothing while its window is shut; an
        // unresolvable schedule keeps a deny in force but drops an allow (#122/#7).
        if !rule_fires(
            effect,
            schedule_state_at(rule.schedule_id, schedules, tz, now),
        ) {
            continue;
        }

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
            // Open Access adds no cards to either set: it is a door-level
            // held-unlock latch, compiled separately by
            // `open_access_hold_until_at` into `CompiledDoor.hold_unlock_until`.
            // Only `allow` is meaningful there; a `deny` open-access rule is
            // inert, so it is correct that this arm ignores the effect bucket.
            DoorRuleKind::OpenAccess => {}
            DoorRuleKind::Card => {
                // rule.value is a card wire-digest at rest (#122/H3).
                bucket.insert(rule.value.clone());
            }
            DoorRuleKind::User => {
                if let Ok(uid) = Uuid::parse_str(&rule.value) {
                    if let Some(cards) = cards_by_user.get(&uid) {
                        for c in cards {
                            bucket.insert(c.clone());
                        }
                    }
                } else {
                    warn!("Door rule of kind=user has non-UUID value '{}'", rule.value);
                }
            }
            DoorRuleKind::Role => {
                let required = match graph.level_of_name(&rule.value) {
                    Some(l) => l,
                    None => {
                        warn!("Door rule of kind=role has unknown role '{}'", rule.value);
                        continue;
                    }
                };
                // Users with no card are absent from cards_by_user and so add
                // nothing -- exactly the previous behaviour.
                for (uid, cards) in cards_by_user {
                    if user_levels.get(uid).copied().unwrap_or(0) >= required {
                        for c in cards {
                            bucket.insert(c.clone());
                        }
                    }
                }
            }
        }
    }

    (allow, deny)
}

#[cfg(test)]
mod schedule_fail_closed_tests {
    use super::{rule_fires, ScheduleState};
    use crate::models::DoorRuleEffect;

    // #120 (#122/#7): losing a schedule must tighten access, never widen it.
    // An unresolvable schedule keeps a deny in force and drops an allow; a
    // definitively-closed window silences both.
    #[test]
    fn an_unresolvable_schedule_keeps_deny_and_drops_allow() {
        assert!(
            rule_fires(DoorRuleEffect::Deny, ScheduleState::Unresolvable),
            "a deny with a deleted/invalid schedule must still deny"
        );
        assert!(
            !rule_fires(DoorRuleEffect::Allow, ScheduleState::Unresolvable),
            "an allow with a deleted/invalid schedule must NOT grant"
        );
    }

    #[test]
    fn active_fires_both_and_inactive_silences_both() {
        assert!(rule_fires(DoorRuleEffect::Allow, ScheduleState::Active));
        assert!(rule_fires(DoorRuleEffect::Deny, ScheduleState::Active));
        assert!(!rule_fires(DoorRuleEffect::Allow, ScheduleState::Inactive));
        assert!(!rule_fires(DoorRuleEffect::Deny, ScheduleState::Inactive));
    }
}
