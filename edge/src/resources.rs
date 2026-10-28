//! One cache for every resource this edge coordinates (#182, step 3; #191).
//!
//! A door, a tool and a room are the same thing to a device: a resource a card
//! is presented to, whose bindings decide what follows a yes. Until this step
//! the edge kept them in two caches with two decision functions, two reply
//! shapes and two loops, and the one place a resource-shaped scan arrived it
//! had to guess which cache to ask. This is the one place now:
//!
//! - [`ResourceCache::resolve`] names a resource by UUID or external id and
//!   says what it is -- kind, session profile, strike pulse -- from whatever
//!   the server has sent about it.
//! - [`ResourceCache::decide`] is the one decision for a badge at anything.
//! - [`ResourceCache::open_session`] opens a session with the resource's
//!   profile: a `momentary` session ends when the strike pulse does, an
//!   `until_stopped` one when something stops it.
//! - [`ResourceCache::tick`] is the one loop: every lease the engine decides
//!   and every Open Access hold pulse that is due, from one pass.
//!
//! ## The policy (#191)
//!
//! "Who may use this resource" used to reach this edge in two representations
//! -- per-principal resource lists for tools (`toolguard/sync`) and
//! per-resource card lists for doors (`doors/state`) -- and `decide` evaluated
//! whichever a resource had, in that policy's own order, which was the last
//! place the door/tool split was visible. The server now sends **one** shape,
//! `resource/policy` (`GET /api/resources/policy`): every resource this edge
//! coordinates, each carrying every rule the engine can read -- compiled
//! allow/deny digests and an Open Access hold, a status and an online rule,
//! the kind, profile, pulse and whether it is in use -- plus the principals.
//! The two old payloads are aliases of it for one release: each arrives and
//! is folded into the same store, so a server that still sends only the old
//! pair is decided exactly as one that sends the new one.
//!
//! [`ResourceCache::decide`] is therefore one evaluation with one arm order,
//! whatever the kind, and one reason vocabulary (FIRMWARE.md documents it):
//!
//! 1. a power lockout, which is sticky and fail-secure;
//! 2. a resource that is authored but disabled;
//! 3. the resource's deny list (deny beats everything below);
//! 4. its Open Access hold, by this device's clock;
//! 5. its allow list -- a compiled card rule admits a card the principals
//!    may not even name;
//! 6. a resource the server insists on deciding itself ([`ScanDecision::AskServer`]);
//! 7. the principal the card digests to: unknown, inactive, authorized for
//!    this resource, or not; and, for one that is, the resource's status.
//!
//! A tool has no lists and a door no status, so each kind simply never takes
//! the arms it cannot; the order is the same for both and the reasons are the
//! same words. A name nothing knows is judged against the principals, as it
//! always was, and refuses.
//!
//! Where the rest of the data comes from is unchanged: the wiring snapshot
//! (`module/state`: bindings, kind, session profile) is [`ModuleState`], and
//! the member allow-list is still mirrored into [`ToolGuardState`] from
//! whichever payload carried it, because the kiosk reads that shape over the
//! edge's own WebSocket and the lease engine reconciles from its arrival.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Duration, Utc};
use css_lib::wire::{Principal, ResourcePolicy, ResourcePolicyPayload};
use tracing::warn;
use uuid::Uuid;

use crate::doors::{hold_pulse_ms, DoorStateSnapshot};
use crate::modules::{LeaseAction, ModuleState};
use crate::power::PowerState;
use crate::toolguard::{SyncPayload, SyncTool, SyncUser, ToolGuardState, ToolStatus};

/// A resource's kind, as the server names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tool,
    Door,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Tool => "tool",
            Kind::Door => "door",
        }
    }

    /// The kind a server names; anything that is not a door is a tool, which
    /// is what every resource was before there were kinds.
    fn parse(named: &str) -> Self {
        if named == "door" {
            Kind::Door
        } else {
            Kind::Tool
        }
    }
}

/// How a session on a resource ends (#182).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProfile {
    /// Over when the strike pulse is: the door as we know it.
    Momentary,
    /// Until a stop: the tool as we know it, and the room.
    UntilStopped,
}

impl SessionProfile {
    /// The profile a server names, or the kind's default when it names none
    /// (a server that predates the field sends an empty string).
    pub fn parse(named: &str, kind: Kind) -> Self {
        match named {
            "momentary" => SessionProfile::Momentary,
            "until_stopped" => SessionProfile::UntilStopped,
            _ => match kind {
                Kind::Door => SessionProfile::Momentary,
                Kind::Tool => SessionProfile::UntilStopped,
            },
        }
    }
}

/// What this edge knows about a resource a device named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The resource's UUID as a string, when the edge knows it; otherwise the
    /// name the device used.
    pub id: String,
    pub kind: Kind,
    pub profile: SessionProfile,
    /// The pulse its latch is released for on a yes, if it has a latch.
    pub pulse_ms: Option<i32>,
}

/// The one answer to a badge at anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanDecision {
    /// Yes. `strike_ms` is the pulse the scanning device may release a latch
    /// for, when the resource has one.
    Allow { strike_ms: Option<i32> },
    /// No, with the reason firmware shows and the server records.
    Deny(String),
    /// The server is the authority for this resource (a metered tool under
    /// online actuation): ask it, and fail closed if it cannot be asked.
    AskServer,
}

/// What one pass of the loop wants delivered.
#[derive(Debug, Clone, PartialEq)]
pub enum TickAction {
    /// A lease decision for one power binding.
    Lease(LeaseAction),
    /// An Open Access hold pulse for a door that is held right now.
    Hold { resource: Uuid, pulse_ms: i32 },
}

/// The reasons [`ResourceCache::decide`] refuses with (#191): one vocabulary
/// for every kind, and the words the server's own tool path uses wherever the
/// two share an arm. FIRMWARE.md's table is written from these.
pub mod reason {
    pub const LOCKED_OUT: &str = "Tool is locked out (power)";
    pub const DISABLED: &str = "Door is disabled";
    pub const DENIED_BY_RULE: &str = "Denied by access rule";
    pub const UNKNOWN_CARD: &str = "Unknown card";
    pub const USER_INACTIVE: &str = "User is not active";
    pub const NOT_AUTHORIZED: &str = "Card not authorized";
    /// Followed by the resource's status word, as the wire spells it.
    pub const UNAVAILABLE_PREFIX: &str = "Tool unavailable: ";
}

/// One resource's policy as this edge holds it: the wire shape with its
/// identifiers and instants parsed once, so a decision parses nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: Uuid,
    external_id: Option<String>,
    kind: Kind,
    session_profile: String,
    status: String,
    requires_online: bool,
    enabled: bool,
    pulse_ms: Option<i32>,
    allow: Vec<String>,
    deny: Vec<String>,
    hold_until: Option<DateTime<Utc>>,
}

impl Entry {
    fn named(&self, name: &str) -> bool {
        self.external_id.as_deref() == Some(name) || self.id.to_string() == name
    }

    /// From the one shape. `None` for an entry whose id is not a UUID, which
    /// the server never sends and this edge cannot address a lease or a hold
    /// to.
    fn from_policy(p: &ResourcePolicy) -> Option<Self> {
        let id = Uuid::parse_str(&p.id).ok()?;
        let hold_until = p.hold_until.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|t| t.with_timezone(&Utc))
        });
        Some(Entry {
            id,
            external_id: p.external_id.clone(),
            kind: Kind::parse(&p.kind),
            session_profile: p.session_profile.clone(),
            status: p.status.clone(),
            requires_online: p.requires_online,
            enabled: p.enabled,
            pulse_ms: p.pulse_ms,
            allow: p.allow.clone(),
            deny: p.deny.clone(),
            hold_until,
        })
    }

    /// From the tool alias (`toolguard/sync`): a tool as the allow-list
    /// describes it, and nothing a tool never had.
    fn from_sync_tool(t: &SyncTool) -> Self {
        Entry {
            id: t.id,
            external_id: t.external_id.clone(),
            kind: Kind::Tool,
            session_profile: String::new(),
            status: status_word(&t.status),
            requires_online: t.requires_online,
            enabled: true,
            pulse_ms: None,
            allow: Vec::new(),
            deny: Vec::new(),
            hold_until: None,
        }
    }

    /// From the door alias (`doors/state`): a door as the compiled snapshot
    /// describes it.
    fn from_compiled_door(d: &crate::doors::CompiledDoor) -> Self {
        Entry {
            id: d.id,
            external_id: None,
            kind: Kind::Door,
            session_profile: String::new(),
            status: String::new(),
            requires_online: false,
            enabled: d.enabled,
            pulse_ms: Some(d.unlock_duration_ms),
            allow: d.allow_cards.clone(),
            deny: d.deny_cards.clone(),
            hold_until: d.hold_unlock_until,
        }
    }

    /// The tool the kiosk's allow-list shape shows for this entry.
    fn to_sync_tool(&self, name: &str) -> SyncTool {
        SyncTool {
            id: self.id,
            external_id: self.external_id.clone(),
            name: name.to_string(),
            status: parse_status(&self.status),
            requires_online: self.requires_online,
        }
    }
}

/// A tool status as the wire spells it (`in_use`, not `InUse`).
fn status_word(status: &ToolStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The wire spelling back to the enum. A word this edge does not know is
/// `retired`: unavailable, which is the direction a surprise should fail in.
fn parse_status(word: &str) -> ToolStatus {
    serde_json::from_value(serde_json::Value::String(word.to_string()))
        .unwrap_or(ToolStatus::Retired)
}

/// A principal as this edge holds one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    digest: String,
    full_name: String,
    is_active: bool,
    authorized: Vec<Uuid>,
}

impl Member {
    fn from_principal(p: &Principal) -> Self {
        Member {
            digest: p.digest.clone(),
            full_name: p.full_name.clone(),
            is_active: p.is_active,
            authorized: p
                .authorized
                .iter()
                .filter_map(|id| Uuid::parse_str(id).ok())
                .collect(),
        }
    }

    fn from_sync_user(u: &SyncUser) -> Self {
        Member {
            digest: u.profile_field_digest.clone(),
            full_name: u.full_name.clone(),
            is_active: u.is_active,
            authorized: u.authorized_tool_ids.clone(),
        }
    }

    fn to_sync_user(&self) -> SyncUser {
        SyncUser {
            profile_field_digest: self.digest.clone(),
            full_name: self.full_name.clone(),
            is_active: self.is_active,
            authorized_tool_ids: self.authorized.clone(),
        }
    }
}

/// The policy store: every resource this edge coordinates and every principal,
/// whichever payload carried them, plus the names the kiosk shape wants back.
#[derive(Debug, Default)]
struct Policy {
    entries: Vec<Entry>,
    names: HashMap<Uuid, String>,
    members: Vec<Member>,
    /// The server stamp of the last door-bearing payload applied (`as_of` of
    /// a policy, `snapshot_at` of a door snapshot). #120 (#123/L2): a config
    /// payload carries no per-door expiry, so a replayed *older* one could
    /// re-add a removed door, revert a deny entry or flip `enabled` back on.
    /// Stamps are monotonic per server, so a strictly older one is dropped.
    applied_at: Option<DateTime<Utc>>,
}

impl Policy {
    fn is_stale(&self, stamp: DateTime<Utc>) -> bool {
        self.applied_at.is_some_and(|applied| stamp < applied)
    }

    fn replace_kind(&mut self, kind: Kind, with: Vec<Entry>) {
        self.entries.retain(|e| e.kind != kind);
        self.entries.extend(with);
    }

    fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.named(name))
    }

    /// The allow-list shape the kiosk reads and the lease engine reconciles
    /// from, derived from the store so one payload keeps both current.
    fn to_sync(&self, device_id: Uuid, profile_field: &str) -> SyncPayload {
        SyncPayload {
            device_id,
            profile_field: profile_field.to_string(),
            tools: self
                .entries
                .iter()
                .filter(|e| e.kind == Kind::Tool)
                .map(|e| e.to_sync_tool(self.names.get(&e.id).map(String::as_str).unwrap_or("")))
                .collect(),
            users: self.members.iter().map(Member::to_sync_user).collect(),
        }
    }
}

pub struct ResourceCache {
    wiring: Arc<ModuleState>,
    /// The kiosk's and the lease engine's view of the allow-list, kept in
    /// step with the policy by every ingest; and the digester every decision
    /// hashes a swipe with.
    allow_list: Arc<ToolGuardState>,
    power: Arc<PowerState>,
    policy: RwLock<Policy>,
    /// When each held door was last sent its hold pulse, so one loop running
    /// every second re-sends at the hold cadence rather than every pass.
    hold_sent: RwLock<HashMap<Uuid, DateTime<Utc>>>,
}

impl ResourceCache {
    pub fn new(
        wiring: Arc<ModuleState>,
        allow_list: Arc<ToolGuardState>,
        power: Arc<PowerState>,
    ) -> Arc<Self> {
        Arc::new(Self {
            wiring,
            allow_list,
            power,
            policy: RwLock::new(Policy::default()),
            hold_sent: RwLock::new(HashMap::new()),
        })
    }

    /// The wiring half, for the ingest paths and the liveness notes that
    /// always wrote to it.
    pub fn wiring(&self) -> &Arc<ModuleState> {
        &self.wiring
    }

    // ----- ingest: one shape, and its two aliases ------------------------

    /// The one shape (#191): replace everything. `requested_at` is when this
    /// edge asked for it (`None`: it was pushed), handed on to the lease engine
    /// with the allow-list derived from it, as the sync alias always was.
    pub fn apply_policy(
        &self,
        payload: ResourcePolicyPayload,
        requested_at: Option<DateTime<Utc>>,
    ) {
        let stamp = DateTime::parse_from_rfc3339(&payload.as_of)
            .ok()
            .map(|t| t.with_timezone(&Utc));
        let device_id = Uuid::parse_str(&payload.device_id).unwrap_or_default();
        let sync = {
            let mut w = self.policy.write().expect("policy lock");
            if let Some(stamp) = stamp {
                if w.is_stale(stamp) {
                    warn!(
                        "Ignoring stale resource policy (as_of {} < applied {})",
                        payload.as_of,
                        w.applied_at.expect("stale implies applied")
                    );
                    return;
                }
                w.applied_at = Some(stamp);
            }
            let mut entries = Vec::with_capacity(payload.resources.len());
            let mut names = HashMap::with_capacity(payload.resources.len());
            for r in &payload.resources {
                match Entry::from_policy(r) {
                    Some(e) => {
                        names.insert(e.id, r.name.clone());
                        entries.push(e);
                    }
                    None => warn!("Resource policy names a resource without a UUID: {}", r.id),
                }
            }
            w.entries = entries;
            w.names = names;
            w.members = payload
                .principals
                .iter()
                .map(Member::from_principal)
                .collect();
            w.to_sync(device_id, &payload.profile_field)
        };
        self.allow_list.apply_sync_at(sync, requested_at);
    }

    /// The same, from bytes.
    pub fn apply_policy_bytes(
        &self,
        bytes: &[u8],
        requested_at: Option<DateTime<Utc>>,
    ) -> Result<(), serde_json::Error> {
        let payload: ResourcePolicyPayload = serde_json::from_slice(bytes)?;
        self.apply_policy(payload, requested_at);
        Ok(())
    }

    /// The tool alias (`toolguard/sync`, pushed as `toolguard/state`): the
    /// tools and the principals, replacing the policy's tools and principals
    /// and leaving its doors.
    pub fn apply_sync_alias(&self, payload: SyncPayload, requested_at: Option<DateTime<Utc>>) {
        {
            let mut w = self.policy.write().expect("policy lock");
            let tools: Vec<Entry> = payload.tools.iter().map(Entry::from_sync_tool).collect();
            for t in &payload.tools {
                w.names.insert(t.id, t.name.clone());
            }
            w.replace_kind(Kind::Tool, tools);
            w.members = payload.users.iter().map(Member::from_sync_user).collect();
        }
        self.allow_list.apply_sync_at(payload, requested_at);
    }

    /// The same, from bytes.
    pub fn apply_sync_alias_bytes(
        &self,
        bytes: &[u8],
        requested_at: Option<DateTime<Utc>>,
    ) -> Result<(), serde_json::Error> {
        let payload: SyncPayload = serde_json::from_slice(bytes)?;
        self.apply_sync_alias(payload, requested_at);
        Ok(())
    }

    /// The door alias (`doors/state`): the compiled doors, replacing the
    /// policy's doors and leaving its tools and principals. Dropped when
    /// older than what is applied, as the door cache always did.
    pub fn apply_doors_alias(&self, snapshot: DoorStateSnapshot) {
        let mut w = self.policy.write().expect("policy lock");
        if w.is_stale(snapshot.snapshot_at) {
            warn!(
                "Ignoring stale doors snapshot (snapshot_at {} < applied {})",
                snapshot.snapshot_at,
                w.applied_at.expect("stale implies applied")
            );
            return;
        }
        w.applied_at = Some(snapshot.snapshot_at);
        let doors: Vec<Entry> = snapshot
            .doors
            .iter()
            .map(Entry::from_compiled_door)
            .collect();
        for d in &snapshot.doors {
            w.names.insert(d.id, d.name.clone());
        }
        w.replace_kind(Kind::Door, doors);
    }

    /// How many resources the policy names, for the log line that says a
    /// payload arrived.
    pub fn policy_len(&self) -> usize {
        self.policy.read().map(|p| p.entries.len()).unwrap_or(0)
    }

    // ----- reading ------------------------------------------------------

    /// What this edge knows about `name` -- a UUID or an external id -- from
    /// whatever the server has sent: the wiring snapshot first (it names the
    /// kind and the profile), then the policy (a resource with no wiring is
    /// still a resource). `None` when nothing knows it.
    pub fn resolve(&self, name: &str) -> Option<Resolved> {
        // The wiring's lock is taken and released before the policy's: this
        // function must hold at most ONE lock at a time, and callers that
        // already hold the policy must use `resolved_from` instead. See the
        // note there.
        let wiring = self.wiring.resource(name);
        let policy = self.policy.read().expect("policy lock");
        Self::resolved_from(&policy, wiring, name)
    }

    /// What `resolve` answers, given a policy the caller already holds and the
    /// wiring it already read.
    ///
    /// **This exists because `decide` must not re-enter the policy lock.**
    /// `std::sync::RwLock` is not reentrant for readers in any useful sense: a
    /// second `read()` on a thread that already holds one blocks behind any
    /// writer queued in between, and that writer is waiting for the first read
    /// to end -- a deadlock that wedges the lock for the whole process. The
    /// edge then answers no scan and publishes no lease ever again, while HTTP
    /// keeps working, so it presents as a coordinator that went quiet rather
    /// than as a crash. CI found it on the `lease` stage the first time a
    /// `doors/state` republish landed beside a badge (#191/#192 era).
    fn resolved_from(
        policy: &Policy,
        wiring: Option<crate::modules::ResourceWiring>,
        name: &str,
    ) -> Option<Resolved> {
        if let Some(w) = wiring {
            let kind = Kind::parse(&w.kind);
            let profile = SessionProfile::parse(&w.session_profile, kind);
            // A door's pulse is what the server compiled for it (its strike
            // binding's, or the site default); a tool's is its strike binding's
            // if it has one, which is how a latch on a non-door is pulsed.
            let pulse_ms = policy
                .find(name)
                .and_then(|e| e.pulse_ms)
                .or(w.strike_pulse_ms);
            return Some(Resolved {
                id: w.id,
                kind,
                profile,
                pulse_ms,
            });
        }
        policy.find(name).map(|e| Resolved {
            id: e.id.to_string(),
            kind: e.kind,
            profile: SessionProfile::parse(&e.session_profile, e.kind),
            pulse_ms: e.pulse_ms,
        })
    }

    /// Whether the server insists on deciding this resource itself.
    pub fn requires_online(&self, name: &str) -> bool {
        self.policy
            .read()
            .expect("policy lock")
            .find(name)
            .is_some_and(|e| e.requires_online)
    }

    /// The one decision for a badge at a resource, as of `now`: the arm order
    /// in the module doc, over the policy, whatever the resource's kind.
    ///
    /// The server ships card lists and principals as card wire-digests, so
    /// the swipe is hashed and matched on its digest, never the raw code. No
    /// digester means an empty token, which matches nothing: fail-closed.
    pub fn decide(&self, name: &str, card: &str, now: DateTime<Utc>) -> ScanDecision {
        if self.power.is_locked(name) {
            return ScanDecision::Deny(reason::LOCKED_OUT.to_string());
        }
        // Everything this decision needs from another lock is read BEFORE the
        // policy's, and the policy's is taken exactly once: see
        // [`Self::resolved_from`] for what re-entering it costs.
        let wiring = self.wiring.resource(name);
        let token = self.allow_list.wire_digest_hex(card).unwrap_or_default();
        let policy = self.policy.read().expect("policy lock");
        let entry = policy.find(name);
        let strike_ms = Self::resolved_from(&policy, wiring, name).and_then(|r| r.pulse_ms);
        if let Some(e) = entry {
            if !e.enabled {
                return ScanDecision::Deny(reason::DISABLED.to_string());
            }
            if e.deny.iter().any(|c| c == &token) {
                return ScanDecision::Deny(reason::DENIED_BY_RULE.to_string());
            }
            // Open Access: while the held-unlock window is live by *this
            // device's* clock, any card enters. Comparing to the local clock is
            // what makes the latch self-expire on disconnect (fail-secure +
            // edge-local expiry): a stale payload cannot hold the door open
            // past `hold_until`.
            if e.hold_until.is_some_and(|until| now < until) {
                return ScanDecision::Allow { strike_ms };
            }
            if e.allow.iter().any(|c| c == &token) {
                return ScanDecision::Allow { strike_ms };
            }
            if e.requires_online {
                return ScanDecision::AskServer;
            }
        }
        // #120 (#14 / L1): constant-time digest comparison, on principle --
        // the presented card digest is a credential and `==` short-circuits.
        let member = policy
            .members
            .iter()
            .find(|m| css_lib::ct::constant_time_str_eq(&m.digest, &token));
        let Some(member) = member else {
            return ScanDecision::Deny(reason::UNKNOWN_CARD.to_string());
        };
        if !member.is_active {
            return ScanDecision::Deny(reason::USER_INACTIVE.to_string());
        }
        let Some(e) = entry.filter(|e| member.authorized.contains(&e.id)) else {
            return ScanDecision::Deny(reason::NOT_AUTHORIZED.to_string());
        };
        if !e.status.is_empty() && e.status != "idle" {
            return ScanDecision::Deny(format!("{}{}", reason::UNAVAILABLE_PREFIX, e.status));
        }
        ScanDecision::Allow { strike_ms }
    }

    /// Open a session on a resource after a yes, with the resource's profile
    /// (#182): an `until_stopped` session lasts until a stop; a `momentary` one
    /// ends when the strike pulse does, and leases the resource's power
    /// bindings -- if it has any -- for exactly that long. A momentary resource
    /// with no pulse has a session of no length, which is the door with no
    /// strike and no power binding: nothing to hold on.
    pub fn open_session(&self, name: &str, now: DateTime<Utc>) {
        let resolved = self.resolve(name);
        let ends_at = match resolved.as_ref().map(|r| r.profile) {
            Some(SessionProfile::Momentary) => Some(
                now + Duration::milliseconds(i64::from(
                    resolved
                        .as_ref()
                        .and_then(|r| r.pulse_ms)
                        .unwrap_or(0)
                        .max(0),
                )),
            ),
            _ => None,
        };
        self.wiring.open_session(name, now, ends_at);
    }

    /// End a session on a resource: a stop, from anything bound to it.
    pub fn close_session(&self, name: &str, now: DateTime<Utc>) {
        self.wiring.close_session(name, now);
    }

    /// Hand the server's view of its sessions to the engine (#179): which
    /// tools it says are in use, and which tools it reports on at all -- a
    /// session on anything else is this edge's to end.
    pub fn reconcile(
        &self,
        payload: &SyncPayload,
        now: DateTime<Utc>,
        requested_at: Option<DateTime<Utc>>,
    ) {
        let in_use: Vec<String> = payload
            .tools
            .iter()
            .filter(|t| t.status == ToolStatus::InUse)
            .map(|t| t.id.to_string())
            .collect();
        let tracked: Vec<String> = payload.tools.iter().map(|t| t.id.to_string()).collect();
        // A polled payload is as old as its request; a pushed one is taken as
        // of its arrival, which is the best that can be said of it.
        let as_of = requested_at.unwrap_or(now);
        self.wiring
            .reconcile_sessions(&in_use, &tracked, now, as_of);
    }

    /// The resources that should be held open right now and the momentary
    /// pulse to (re)send each, as of `now`. A disabled resource is never held
    /// (mirrors [`Self::decide`]).
    fn hold_pulses_at(&self, now: DateTime<Utc>, refresh: Duration) -> Vec<(Uuid, i32)> {
        self.policy
            .read()
            .expect("policy lock")
            .entries
            .iter()
            .filter(|e| e.enabled)
            .filter_map(|e| hold_pulse_ms(e.hold_until, now, refresh).map(|ms| (e.id, ms)))
            .collect()
    }

    /// One pass of the one loop: every lease the engine decides this pass, and
    /// a hold pulse for every door whose Open Access window is live and whose
    /// last pulse is `hold_refresh` old. The loop may run every second; the
    /// pulses go out at the cadence the hold was designed for, and a door that
    /// stops being held is forgotten so its next window starts fresh.
    pub fn tick(
        &self,
        now: DateTime<Utc>,
        ttl: Duration,
        module_timeout: Duration,
        hold_refresh: Duration,
    ) -> Vec<TickAction> {
        let mut out: Vec<TickAction> = self
            .wiring
            .tick(now, ttl, module_timeout)
            .into_iter()
            .map(TickAction::Lease)
            .collect();
        let held = self.hold_pulses_at(now, hold_refresh);
        let mut sent = self.hold_sent.write().expect("hold lock");
        sent.retain(|door, _| held.iter().any(|(d, _)| d == door));
        for (door, pulse_ms) in held {
            let due = sent
                .get(&door)
                .is_none_or(|last| now - *last >= hold_refresh);
            if due {
                sent.insert(door, now);
                out.push(TickAction::Hold {
                    resource: door,
                    pulse_ms,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doors::{CompiledDoor, DoorStateSnapshot};
    use css_lib::wire::{DeviceBinding, ToolModuleStatePayload, ToolModuleTool};

    const PEP: &str = "0404040404040404040404040404040404040404040404040404040404040404";
    const CARD: &str = "CARD-0001";

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn digester() -> Arc<css_lib::card_crypto::CardDigester> {
        Arc::new(css_lib::card_crypto::CardDigester::from_hex(PEP).expect("pepper"))
    }

    fn digest(card: &str) -> String {
        hex::encode(digester().wire_digest(card).expect("digests"))
    }

    fn door_id() -> Uuid {
        Uuid::from_u128(0xD00A)
    }

    fn tool_id() -> Uuid {
        Uuid::from_u128(0x7001)
    }

    fn drill_id() -> Uuid {
        Uuid::from_u128(0x7002)
    }

    fn binding(id: &str, role: &str, on_disconnect: &str) -> DeviceBinding {
        DeviceBinding {
            id: id.to_string(),
            device_id: format!("dev-{id}"),
            role: role.to_string(),
            name: id.to_string(),
            params: serde_json::json!({}),
            on_disconnect: on_disconnect.to_string(),
            start_stage: 0,
            start_delay_ms: 0,
            run_on_ms: 0,
            pulse_ms: 0,
            hold_open_access: true,
        }
    }

    fn entry(
        id: Uuid,
        ext: Option<&str>,
        kind: &str,
        profile: &str,
        modules: Vec<DeviceBinding>,
    ) -> ToolModuleTool {
        ToolModuleTool {
            tool_id: id.to_string(),
            external_id: ext.map(str::to_string),
            kind: kind.to_string(),
            session_profile: profile.to_string(),
            in_use: false,
            modules,
            interlocks: vec![],
            power_fails_safe: true,
        }
    }

    /// The wiring every fixture shares, as main.rs builds it: a door with a
    /// coordinator, a strike (4200 ms) and one power binding (the room), and a
    /// tool with a reader and a plug.
    fn wired(door_profile: &str) -> Arc<ModuleState> {
        let wiring = Arc::new(ModuleState::new());
        let mut plug = binding("plug", "power", "ignore");
        plug.device_id = "dev-plug".to_string();
        let mut room_plug = binding("room-plug", "power", "ignore");
        room_plug.device_id = "dev-room-plug".to_string();
        let mut latch = binding("latch", "strike", "ignore");
        latch.pulse_ms = 4200;
        wiring.apply_state(ToolModuleStatePayload {
            as_of: "x".to_string(),
            tools: vec![
                entry(
                    door_id(),
                    None,
                    "door",
                    door_profile,
                    vec![binding("edge", "edge", "fail_off"), latch, room_plug],
                ),
                entry(
                    tool_id(),
                    Some("laser"),
                    "tool",
                    "",
                    vec![binding("reader", "reader", "ignore"), plug],
                ),
            ],
        });
        wiring
    }

    fn empty(wiring: Arc<ModuleState>, pepper: bool) -> Arc<ResourceCache> {
        ResourceCache::new(
            wiring,
            Arc::new(ToolGuardState::with_digester(pepper.then(digester))),
            Arc::new(PowerState::new()),
        )
    }

    fn sync_tool(id: Uuid, ext: &str, status: ToolStatus) -> SyncTool {
        SyncTool {
            id,
            external_id: Some(ext.to_string()),
            name: ext.to_string(),
            status,
            requires_online: false,
        }
    }

    /// The member's allow-list as the sync alias carries it: the member opens
    /// the laser; the drill is named by the allow-list and wired to nothing.
    fn sync() -> SyncPayload {
        SyncPayload {
            device_id: Uuid::from_u128(0xE),
            profile_field: "card".to_string(),
            tools: vec![
                sync_tool(tool_id(), "laser", ToolStatus::Idle),
                sync_tool(drill_id(), "drill", ToolStatus::Idle),
            ],
            users: vec![SyncUser {
                profile_field_digest: digest(CARD),
                full_name: "M".to_string(),
                is_active: true,
                authorized_tool_ids: vec![tool_id()],
            }],
        }
    }

    fn compiled_door(allow: &[&str], deny: &[&str], hold: Option<DateTime<Utc>>) -> CompiledDoor {
        CompiledDoor {
            id: door_id(),
            name: "Front".to_string(),
            enabled: true,
            unlock_duration_ms: 4200,
            allow_cards: allow.iter().map(|s| s.to_string()).collect(),
            deny_cards: deny.iter().map(|s| s.to_string()).collect(),
            hold_unlock_until: hold,
        }
    }

    fn doors(at: DateTime<Utc>, door: CompiledDoor) -> DoorStateSnapshot {
        DoorStateSnapshot {
            snapshot_at: at,
            doors: vec![door],
        }
    }

    /// The one cache fed by the two ALIASES, as a server that predates #191
    /// feeds it: the member's card opens the door (compiled) and the laser
    /// (allow-list).
    fn cache(door_profile: &str) -> Arc<ResourceCache> {
        let c = empty(wired(door_profile), true);
        c.apply_doors_alias(doors(t0(), compiled_door(&[&digest(CARD)], &[], None)));
        c.apply_sync_alias(sync(), None);
        c
    }

    fn policy_resource(id: Uuid, ext: Option<&str>, kind: &str) -> ResourcePolicy {
        ResourcePolicy {
            id: id.to_string(),
            external_id: ext.map(str::to_string),
            name: ext.unwrap_or("Front").to_string(),
            kind: kind.to_string(),
            session_profile: String::new(),
            status: if kind == "tool" {
                "idle".to_string()
            } else {
                String::new()
            },
            requires_online: false,
            in_use: false,
            enabled: true,
            pulse_ms: (kind == "door").then_some(4200),
            allow: Vec::new(),
            deny: Vec::new(),
            hold_until: None,
        }
    }

    /// The same facts in the ONE shape (#191).
    fn policy() -> ResourcePolicyPayload {
        let mut door = policy_resource(door_id(), None, "door");
        door.allow = vec![digest(CARD)];
        ResourcePolicyPayload {
            device_id: Uuid::from_u128(0xE).to_string(),
            as_of: t0().to_rfc3339(),
            profile_field: "card".to_string(),
            resources: vec![
                policy_resource(tool_id(), Some("laser"), "tool"),
                policy_resource(drill_id(), Some("drill"), "tool"),
                door,
            ],
            principals: vec![Principal {
                digest: digest(CARD),
                full_name: "M".to_string(),
                is_active: true,
                authorized: vec![tool_id().to_string()],
            }],
        }
    }

    /// The one cache fed by the one shape.
    fn policy_cache(door_profile: &str) -> Arc<ResourceCache> {
        let c = empty(wired(door_profile), true);
        c.apply_policy(policy(), None);
        c
    }

    fn leases_for(actions: &[TickAction], device: &str) -> Vec<bool> {
        actions
            .iter()
            .filter_map(|a| match a {
                TickAction::Lease(l) if l.device_id == device => Some(l.grant),
                _ => None,
            })
            .collect()
    }

    fn allow(strike_ms: Option<i32>) -> ScanDecision {
        ScanDecision::Allow { strike_ms }
    }

    fn deny(why: &str) -> ScanDecision {
        ScanDecision::Deny(why.to_string())
    }

    fn door() -> String {
        door_id().to_string()
    }

    #[test]
    fn a_door_and_a_tool_are_resolved_by_either_name_with_their_kind_and_profile() {
        for c in [cache(""), policy_cache("")] {
            let door = c.resolve(&door()).expect("door");
            assert_eq!(door.kind, Kind::Door);
            assert_eq!(
                door.profile,
                SessionProfile::Momentary,
                "a door defaults to momentary"
            );
            assert_eq!(door.pulse_ms, Some(4200));
            let tool = c.resolve("laser").expect("tool by external id");
            assert_eq!(tool.kind, Kind::Tool);
            assert_eq!(tool.profile, SessionProfile::UntilStopped);
            assert_eq!(tool.id, tool_id().to_string());
            assert_eq!(c.resolve("nothing-here"), None);
            let unwired = c
                .resolve("drill")
                .expect("a tool the policy names and nothing wires");
            assert_eq!(unwired.kind, Kind::Tool);
            assert_eq!(unwired.id, drill_id().to_string());
            assert_eq!(unwired.pulse_ms, None);
        }
        for c in [cache("until_stopped"), policy_cache("until_stopped")] {
            assert_eq!(
                c.resolve(&door()).unwrap().profile,
                SessionProfile::UntilStopped,
                "the server's profile outranks the kind's default"
            );
        }
    }

    #[test]
    fn a_resource_the_wiring_has_not_named_is_resolved_from_the_policy_alone() {
        // No wiring at all: a restarted edge whose module/state has not come
        // back yet still knows what the policy names, with the policy's pulse
        // and the kind's default profile.
        let c = empty(Arc::new(ModuleState::new()), true);
        c.apply_policy(policy(), None);
        let door = c.resolve(&door()).expect("door from the policy");
        assert_eq!(door.kind, Kind::Door);
        assert_eq!(door.pulse_ms, Some(4200));
        assert_eq!(door.profile, SessionProfile::Momentary);
        let tool = c.resolve("laser").expect("tool from the policy");
        assert_eq!(tool.kind, Kind::Tool);
        assert_eq!(tool.pulse_ms, None);
    }

    #[test]
    fn one_decide_answers_a_door_with_its_pulse_and_a_tool_without_one() {
        for c in [cache(""), policy_cache("")] {
            assert_eq!(c.decide(&door(), CARD, t0()), allow(Some(4200)));
            assert_eq!(c.decide("laser", CARD, t0()), allow(None));
            // One vocabulary (#191): a card nothing names is `Unknown card`
            // at a door as at a tool, and a known card with no grant is
            // `Card not authorized` at either.
            assert_eq!(
                c.decide(&door(), "NOBODY", t0()),
                deny(reason::UNKNOWN_CARD)
            );
            assert_eq!(
                c.decide("laser", "NOBODY", t0()),
                deny(reason::UNKNOWN_CARD)
            );
            assert_eq!(
                c.decide("drill", CARD, t0()),
                deny(reason::NOT_AUTHORIZED),
                "a tool the member is not listed for"
            );
            assert_eq!(
                c.decide("not-a-resource", CARD, t0()),
                deny(reason::NOT_AUTHORIZED),
                "a name nothing knows is judged against the principals, as it always was"
            );
        }
    }

    #[test]
    fn the_one_shape_and_its_two_aliases_decide_identically() {
        // The property the aliases exist for: a server still sending the old
        // pair is decided exactly as one sending the new shape. Every arm the
        // fixtures can reach, both ways.
        let a = cache("");
        let b = policy_cache("");
        for (name, card) in [
            (door(), CARD),
            (door(), "NOBODY"),
            ("laser".to_string(), CARD),
            ("laser".to_string(), "NOBODY"),
            ("drill".to_string(), CARD),
            ("ghost".to_string(), CARD),
        ] {
            assert_eq!(
                a.decide(&name, card, t0()),
                b.decide(&name, card, t0()),
                "{name} / {card}"
            );
        }
        // And the kiosk's shape is derived identically from either.
        let from_alias = a.allow_list.get_state().expect("alias state");
        let from_policy = b.allow_list.get_state().expect("policy state");
        assert_eq!(
            serde_json::to_value(&from_alias).unwrap(),
            serde_json::to_value(&from_policy).unwrap()
        );
    }

    #[test]
    fn the_policy_feeds_the_kiosk_shape_and_the_lease_engine() {
        // The allow-list the kiosk reads over the edge's WebSocket, and the
        // arrival the lease engine reconciles from, come from the policy now.
        let (allow_list, rx) = ToolGuardState::new_with_notify(Some(digester()));
        let allow_list = Arc::new(allow_list);
        let c = ResourceCache::new(
            Arc::new(ModuleState::new()),
            allow_list,
            Arc::new(PowerState::new()),
        );
        let asked = t0() - Duration::seconds(1);
        c.apply_policy(policy(), Some(asked));
        let arrival = rx.try_recv().expect("the lease engine is told");
        assert_eq!(arrival.requested_at, Some(asked));
        assert_eq!(arrival.payload.tools.len(), 2, "the tools, not the door");
        assert_eq!(arrival.payload.tools[0].name, "laser");
        assert_eq!(arrival.payload.tools[0].status, ToolStatus::Idle);
        assert_eq!(arrival.payload.users.len(), 1);
        assert_eq!(
            arrival.payload.users[0].authorized_tool_ids,
            vec![tool_id()]
        );
        assert_eq!(arrival.payload.profile_field, "card");
    }

    #[test]
    fn a_status_word_the_edge_does_not_know_reads_as_unavailable() {
        let mut p = policy();
        p.resources[0].status = "liquefied".to_string();
        let c = empty(wired(""), true);
        c.apply_policy(p, None);
        assert_eq!(
            c.decide("laser", CARD, t0()),
            deny("Tool unavailable: liquefied"),
            "the word is shown as sent"
        );
        assert_eq!(
            c.allow_list.get_state().unwrap().tools[0].status,
            ToolStatus::Retired,
            "and the kiosk shape fails toward unavailable"
        );
    }

    #[test]
    fn a_resource_without_a_uuid_is_dropped_not_trusted() {
        let mut p = policy();
        p.resources[0].id = "laser".to_string();
        let c = empty(Arc::new(ModuleState::new()), true);
        c.apply_policy(p, None);
        assert_eq!(c.policy_len(), 2);
        assert_eq!(c.resolve("laser"), None);
    }

    // ----- the arms, in order, each proven to fire ------------------------

    #[test]
    fn a_power_lockout_refuses_before_anything_else() {
        let c = cache("");
        c.power.apply_state(css_lib::wire::PowerStatePayload {
            as_of: "x".to_string(),
            locked_tool_ids: vec!["laser".to_string()],
            circuits: vec![],
            tools: vec![],
        });
        assert_eq!(c.decide("laser", CARD, t0()), deny(reason::LOCKED_OUT));
    }

    #[test]
    fn a_disabled_resource_refuses_a_card_its_rules_allow() {
        let mut d = compiled_door(&[&digest(CARD)], &[], None);
        d.enabled = false;
        let c = empty(wired(""), true);
        c.apply_doors_alias(doors(t0(), d));
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::DISABLED));
        let mut p = policy();
        p.resources[2].enabled = false;
        let c = empty(wired(""), true);
        c.apply_policy(p, None);
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::DISABLED));
    }

    #[test]
    fn deny_wins_even_when_the_card_is_also_allowed_and_even_during_a_hold() {
        // The single most important property of the decision: a card in both
        // lists must be refused, and so must a banned card while the door is
        // held for everyone else. Getting this backwards opens a door to
        // someone explicitly barred from it.
        let held = Some(t0() + Duration::hours(1));
        let c = empty(wired(""), true);
        c.apply_doors_alias(doors(
            t0(),
            compiled_door(&[&digest(CARD), &digest("B2")], &[&digest(CARD)], held),
        ));
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::DENIED_BY_RULE));
        assert_eq!(c.decide(&door(), "B2", t0()), allow(Some(4200)));
        assert_eq!(
            c.decide(&door(), "STRANGER", t0()),
            allow(Some(4200)),
            "the hold admits anyone not banned"
        );
    }

    #[test]
    fn open_access_admits_any_card_while_live_and_self_expires_from_the_local_clock() {
        // The fail-secure guarantee: once `now` passes the hold, the latch is
        // dead even though no fresh payload (no closing push) ever arrived.
        let c = empty(wired(""), true);
        c.apply_doors_alias(doors(
            t0(),
            compiled_door(&[], &[], Some(t0() + Duration::minutes(10))),
        ));
        assert_eq!(c.decide(&door(), "STRANGER", t0()), allow(Some(4200)));
        assert_eq!(
            c.decide(&door(), "STRANGER", t0() + Duration::minutes(10)),
            deny(reason::UNKNOWN_CARD)
        );
        // The same window, in the one shape.
        let mut p = policy();
        p.resources[2].hold_until = Some((t0() + Duration::minutes(10)).to_rfc3339());
        let c = empty(wired(""), true);
        c.apply_policy(p, None);
        assert_eq!(c.decide(&door(), "STRANGER", t0()), allow(Some(4200)));
        assert_eq!(
            c.decide(&door(), "STRANGER", t0() + Duration::minutes(10)),
            deny(reason::UNKNOWN_CARD)
        );
    }

    #[test]
    fn a_compiled_card_rule_admits_a_card_the_principals_do_not_name() {
        // A door's allow list is compiled from its rules, and a card rule can
        // name a card that is nobody's -- a guest fob. The list is the grant.
        let c = empty(wired(""), true);
        c.apply_doors_alias(doors(t0(), compiled_door(&[&digest("GUEST")], &[], None)));
        assert_eq!(c.decide(&door(), "GUEST", t0()), allow(Some(4200)));
    }

    #[test]
    fn a_resource_the_server_decides_itself_is_asked_not_decided() {
        let mut s = sync();
        s.tools[0].requires_online = true;
        let c = empty(wired(""), true);
        c.apply_sync_alias(s, None);
        assert!(c.requires_online("laser"));
        assert!(c.requires_online(&tool_id().to_string()));
        assert!(!c.requires_online("drill"));
        assert!(!c.requires_online("unknown"));
        assert_eq!(c.decide("laser", CARD, t0()), ScanDecision::AskServer);
        assert_eq!(
            c.decide("laser", "NOBODY", t0()),
            ScanDecision::AskServer,
            "even an unknown card: the server is the authority"
        );
    }

    #[test]
    fn an_inactive_member_is_refused_before_their_grants_are_read() {
        let mut p = policy();
        p.principals[0].is_active = false;
        let c = empty(wired(""), true);
        c.apply_policy(p, None);
        assert_eq!(c.decide("laser", CARD, t0()), deny(reason::USER_INACTIVE));
        assert_eq!(
            c.decide(&door(), CARD, t0()),
            allow(Some(4200)),
            "but a compiled card list is a grant in its own right"
        );
    }

    #[test]
    fn a_non_idle_tool_is_unavailable_and_names_its_status_as_the_wire_spells_it() {
        for (st, word) in [
            (ToolStatus::InUse, "in_use"),
            (ToolStatus::Maintenance, "maintenance"),
            (ToolStatus::Broken, "broken"),
            (ToolStatus::Repair, "repair"),
            (ToolStatus::Retired, "retired"),
        ] {
            let mut s = sync();
            s.tools[0].status = st;
            let c = empty(wired(""), true);
            c.apply_sync_alias(s, None);
            assert_eq!(
                c.decide("laser", CARD, t0()),
                deny(&format!("Tool unavailable: {word}"))
            );
        }
    }

    // ----- the credential is the digest, never the card ---------------------

    #[test]
    fn an_edge_without_a_pepper_authorizes_nobody() {
        // No pepper means no way to hash a swipe, so it must refuse rather than
        // compare raw values. Falling back would silently restore exactly the
        // behaviour #109 removed, and would do so precisely when somebody had
        // misconfigured the device -- the worst moment to be lenient.
        let c = empty(wired(""), false);
        c.apply_policy(policy(), None);
        assert_eq!(c.decide("laser", CARD, t0()), deny(reason::UNKNOWN_CARD));
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::UNKNOWN_CARD));
    }

    #[test]
    fn presenting_the_digest_instead_of_the_card_is_refused() {
        // A payload carrying digests must not authorize somebody who presents
        // the digest itself -- which is what a stolen device's cache would
        // hand an attacker.
        let c = policy_cache("");
        assert_eq!(c.decide("laser", CARD, t0()), allow(None), "precondition");
        assert_eq!(
            c.decide("laser", &digest(CARD), t0()),
            deny(reason::UNKNOWN_CARD)
        );
        assert_eq!(
            c.decide(&door(), &digest(CARD), t0()),
            deny(reason::UNKNOWN_CARD)
        );
    }

    #[test]
    fn cold_start_denies_to_be_safe() {
        // No policy loaded yet: deny rather than energize or release. This is
        // the fail-secure property, and the negative control that keeps every
        // "authorized" case above from being vacuous.
        let c = empty(wired(""), true);
        assert_eq!(c.decide("laser", CARD, t0()), deny(reason::UNKNOWN_CARD));
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::UNKNOWN_CARD));
        assert_eq!(c.policy_len(), 0);
    }

    // ----- ingest: staleness and the aliases' halves --------------------------

    #[test]
    fn a_stale_payload_is_ignored_but_a_newer_one_applies() {
        // #120 (#123/L2): a replayed *older* config payload must not revert the
        // cache (re-allow a denied card, resurrect a removed door). A strictly
        // newer one must still take effect. Two-sided: remove the freshness
        // check and the "still denied" assertion fails.
        let at = |secs: i64| t0() + Duration::seconds(secs);
        let c = empty(wired(""), true);
        c.apply_doors_alias(doors(at(60), compiled_door(&[], &[&digest(CARD)], None)));
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::DENIED_BY_RULE));
        // An OLDER snapshot that would re-allow the card: ignored.
        c.apply_doors_alias(doors(at(0), compiled_door(&[&digest(CARD)], &[], None)));
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::DENIED_BY_RULE));
        // An older POLICY likewise.
        let mut p = policy();
        p.as_of = at(30).to_rfc3339();
        c.apply_policy(p, None);
        assert_eq!(c.decide(&door(), CARD, t0()), deny(reason::DENIED_BY_RULE));
        assert_eq!(
            c.resolve("drill"),
            None,
            "nothing of the stale policy landed (the drill is wired to nothing)"
        );
        // A genuinely newer policy IS applied.
        let mut p = policy();
        p.as_of = at(120).to_rfc3339();
        c.apply_policy(p, None);
        assert_eq!(c.decide(&door(), CARD, t0()), allow(Some(4200)));
    }

    #[test]
    fn each_alias_replaces_its_own_half_and_leaves_the_other() {
        let c = cache("");
        // A new sync without the drill: the drill is gone, the door is not.
        let mut s = sync();
        s.tools.truncate(1);
        c.apply_sync_alias(s, None);
        assert_eq!(c.resolve("drill"), None);
        assert_eq!(c.decide(&door(), CARD, t0()), allow(Some(4200)));
        // A new door snapshot with no doors: the door is gone, the laser is not.
        c.apply_doors_alias(DoorStateSnapshot {
            snapshot_at: t0() + Duration::seconds(1),
            doors: vec![],
        });
        assert_eq!(c.resolve("laser").map(|r| r.kind), Some(Kind::Tool));
        assert_eq!(
            c.decide(&door(), CARD, t0()),
            deny(reason::NOT_AUTHORIZED),
            "a door the policy no longer names is judged against the principals"
        );
    }

    // ----- sessions and the loop --------------------------------------------

    #[test]
    fn a_momentary_door_session_leases_its_power_only_for_the_pulse() {
        let c = cache("");
        let ttl = Duration::seconds(3);
        let timeout = Duration::seconds(120);
        let refresh = Duration::seconds(30);
        let door = door();
        // Nobody signed on: refused.
        let before = c.tick(t0(), ttl, timeout, refresh);
        assert_eq!(leases_for(&before, "dev-room-plug"), vec![false]);
        // A badge: the session lasts as long as the 4200 ms pulse.
        assert!(matches!(
            c.decide(&door, CARD, t0()),
            ScanDecision::Allow { .. }
        ));
        c.open_session(&door, t0());
        let during = c.tick(t0() + Duration::seconds(1), ttl, timeout, refresh);
        assert_eq!(leases_for(&during, "dev-room-plug"), vec![true]);
        let after = c.tick(t0() + Duration::seconds(5), ttl, timeout, refresh);
        assert_eq!(leases_for(&after, "dev-room-plug"), vec![false]);
        let reason = after.iter().find_map(|a| match a {
            TickAction::Lease(l) if l.device_id == "dev-room-plug" => l.reason.clone(),
            _ => None,
        });
        assert_eq!(reason.as_deref(), Some("NotInUse"));
    }

    #[test]
    fn an_until_stopped_door_is_the_room_leased_until_a_stop_whatever_the_sync_says() {
        let c = cache("until_stopped");
        let ttl = Duration::seconds(3);
        let timeout = Duration::seconds(120);
        let refresh = Duration::seconds(30);
        let door = door();
        c.open_session(&door, t0());
        let during = c.tick(t0() + Duration::seconds(1), ttl, timeout, refresh);
        assert_eq!(leases_for(&during, "dev-room-plug"), vec![true]);
        // Well past any pulse and any grace, and a sync that says nothing about
        // the door: still leased, because the server does not track a door's
        // session through the allow-list and this edge is the one that ends it.
        let mut s = sync();
        s.users.clear();
        c.reconcile(&s, t0() + Duration::minutes(10), None);
        let later = c.tick(t0() + Duration::minutes(10), ttl, timeout, refresh);
        assert_eq!(leases_for(&later, "dev-room-plug"), vec![true]);
        // The stop ends it at once.
        c.close_session(&door, t0() + Duration::minutes(10));
        let stopped = c.tick(
            t0() + Duration::minutes(10) + Duration::seconds(1),
            ttl,
            timeout,
            refresh,
        );
        assert_eq!(leases_for(&stopped, "dev-room-plug"), vec![false]);
    }

    #[test]
    fn a_tool_session_the_server_stopped_is_retired_after_the_grace_as_before() {
        let c = cache("");
        let ttl = Duration::seconds(3);
        let timeout = Duration::seconds(120);
        let refresh = Duration::seconds(30);
        c.open_session("laser", t0());
        assert_eq!(
            leases_for(
                &c.tick(t0() + Duration::seconds(1), ttl, timeout, refresh),
                "dev-plug"
            ),
            vec![true]
        );
        let mut sync_idle = sync();
        sync_idle.users.clear();
        // Within the grace the local session outranks the stale sync.
        c.reconcile(&sync_idle, t0() + Duration::seconds(5), None);
        assert_eq!(
            leases_for(
                &c.tick(t0() + Duration::seconds(5), ttl, timeout, refresh),
                "dev-plug"
            ),
            vec![true]
        );
        // Past it, a sync saying idle about a TRACKED tool retires it.
        c.reconcile(&sync_idle, t0() + Duration::seconds(60), None);
        assert_eq!(
            leases_for(
                &c.tick(t0() + Duration::seconds(60), ttl, timeout, refresh),
                "dev-plug"
            ),
            vec![false]
        );
    }

    #[test]
    fn one_tick_carries_hold_pulses_at_the_hold_cadence_not_every_pass() {
        let c = cache("");
        c.apply_doors_alias(doors(
            t0() + Duration::seconds(1),
            compiled_door(&[], &[], Some(t0() + Duration::minutes(10))),
        ));
        let ttl = Duration::seconds(3);
        let timeout = Duration::seconds(120);
        let refresh = Duration::seconds(30);
        let holds = |actions: &[TickAction]| {
            actions
                .iter()
                .filter(|a| matches!(a, TickAction::Hold { .. }))
                .count()
        };
        assert_eq!(
            holds(&c.tick(t0(), ttl, timeout, refresh)),
            1,
            "first pass pulses"
        );
        assert_eq!(
            holds(&c.tick(t0() + Duration::seconds(1), ttl, timeout, refresh)),
            0
        );
        assert_eq!(
            holds(&c.tick(t0() + Duration::seconds(29), ttl, timeout, refresh)),
            0
        );
        assert_eq!(
            holds(&c.tick(t0() + Duration::seconds(30), ttl, timeout, refresh)),
            1
        );
        // The window ends: no pulse, and the door is forgotten so a new window
        // starts fresh.
        assert_eq!(
            holds(&c.tick(t0() + Duration::minutes(11), ttl, timeout, refresh)),
            0
        );
        assert!(c.hold_sent.read().unwrap().is_empty());
    }

    /// A badge decided while a snapshot is landing must not wedge the cache.
    ///
    /// The defect this guards is a deadlock, not a wrong answer: `decide` held
    /// the policy's read lock and then called `resolve`, which took it again.
    /// `std::sync::RwLock` queues a waiting writer between those two reads, and
    /// the second read waits for that writer while the writer waits for the
    /// first read -- so the whole lock is wedged, for good. Every later scan,
    /// every lease pass and every ingest blocks, and because the edge's HTTP
    /// side keeps answering, it presents as a coordinator that silently stopped
    /// coordinating. A real `doors/state` republish beside a badge is exactly
    /// what produced it, in CI, with the local battery green twice.
    ///
    /// Written as contention on purpose: there is no single-threaded order of
    /// calls that shows it. The watchdog is what makes it a test rather than a
    /// hang -- on the defect this returns `Err(RecvTimeout)` instead of
    /// blocking the suite forever.
    #[test]
    fn a_decision_during_a_snapshot_does_not_wedge_the_policy_lock() {
        let c = cache("until_stopped");
        let writer = Arc::clone(&c);
        let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
        let wtx = tx.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wstop = Arc::clone(&stop);
        // The ingest side: a door snapshot landing over and over, each one
        // newer than the last so none is dropped as stale. It runs until the
        // reader is done, so a writer is queued at every moment the reader
        // could re-enter the lock.
        let w = std::thread::spawn(move || {
            let mut i = 0i64;
            while !wstop.load(std::sync::atomic::Ordering::Relaxed) {
                writer.apply_doors_alias(doors(
                    t0() + Duration::milliseconds(i),
                    compiled_door(&[&digest(CARD)], &[], None),
                ));
                i += 1;
            }
            let _ = wtx.send("writer");
        });
        // The decision side. Few iterations on purpose: one `decide` hashes the
        // swipe with argon2id, measured at ~680 ms in a debug build, and on the
        // defect the lock was held across exactly that -- so the window a
        // writer had to arrive in was enormous and a handful of badges is
        // ample. The cheap readers between them are the lease engine's pass and
        // the scan path's resolve, which the same wedge also stops.
        let reader = Arc::clone(&c);
        let r = std::thread::spawn(move || {
            let ttl = Duration::seconds(3);
            let timeout = Duration::seconds(120);
            let refresh = Duration::seconds(30);
            for _ in 0..8 {
                let _ = reader.decide(&door(), CARD, t0());
                for _ in 0..50 {
                    let _ = reader.resolve(&door());
                    let _ = reader.requires_online("laser");
                    let _ = reader.tick(t0(), ttl, timeout, refresh);
                }
            }
            let _ = tx.send("reader");
        });

        let mut done = 0;
        while done < 2 {
            match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                Ok("reader") => {
                    // Tell the writer to stop only once the reader is through,
                    // so contention never lets up while it matters.
                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    done += 1;
                }
                Ok(_) => done += 1,
                Err(e) => panic!(
                    "a thread never finished ({e}): the policy lock is wedged. \
                     `decide` or `tick` is taking the policy lock twice on one \
                     thread, or holding it across another lock -- see \
                     ResourceCache::resolved_from."
                ),
            }
        }
        w.join().expect("writer thread panicked");
        r.join().expect("reader thread panicked");
        // And the cache still answers afterwards, so "finished" is not "gave up".
        assert_eq!(c.decide(&door(), CARD, t0()), allow(Some(4200)));
    }

    #[test]
    fn a_held_but_disabled_door_is_never_driven_open() {
        let held = Some(t0() + Duration::hours(1));
        let mut shut = compiled_door(&[], &[], held);
        shut.enabled = false;
        let c = empty(wired(""), true);
        c.apply_doors_alias(doors(t0(), shut));
        let refresh = Duration::seconds(30);
        assert!(c.hold_pulses_at(t0(), refresh).is_empty());
        // And the same door enabled is the positive control.
        c.apply_doors_alias(doors(
            t0() + Duration::seconds(1),
            compiled_door(&[], &[], held),
        ));
        assert_eq!(c.hold_pulses_at(t0(), refresh), vec![(door_id(), 45_000)]);
    }
}
