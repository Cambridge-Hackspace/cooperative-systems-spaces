//! One cache for every resource this edge coordinates (#182, step 3).
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
//! What it is made of is honest about where the data still comes from. The
//! server sends three things and this edge ingests each where it always did:
//! the wiring snapshot (`module/state`: bindings, kind, session profile) into
//! [`ModuleState`], the compiled door policy (`doors/state`: card lists and the
//! Open Access window) into [`DoorsState`], and the member allow-list
//! (`toolguard/sync`: principals and tool status) into [`ToolGuardState`]. The
//! door policy and the tool policy are two representations of "who may" --
//! per-resource card lists versus per-principal resource lists -- so the
//! decision evaluates whichever a resource has, in that policy's own order, and
//! both answer the same [`ScanDecision`]. One policy shape on the wire is the
//! server's side of the convergence; this is the edge's.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use crate::doors::{Decision as DoorDecision, DoorsState};
use crate::modules::{LeaseAction, ModuleState};
use crate::power::PowerState;
use crate::toolguard::{AccessResult, SyncPayload, ToolGuardState, ToolStatus};

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

pub struct ResourceCache {
    wiring: Arc<ModuleState>,
    doors: Arc<DoorsState>,
    allow_list: Arc<ToolGuardState>,
    power: Arc<PowerState>,
    /// When each held door was last sent its hold pulse, so one loop running
    /// every second re-sends at the hold cadence rather than every pass.
    hold_sent: RwLock<HashMap<Uuid, DateTime<Utc>>>,
}

impl ResourceCache {
    pub fn new(
        wiring: Arc<ModuleState>,
        doors: Arc<DoorsState>,
        allow_list: Arc<ToolGuardState>,
        power: Arc<PowerState>,
    ) -> Arc<Self> {
        Arc::new(Self {
            wiring,
            doors,
            allow_list,
            power,
            hold_sent: RwLock::new(HashMap::new()),
        })
    }

    /// The wiring half, for the ingest paths and the liveness notes that
    /// always wrote to it.
    pub fn wiring(&self) -> &Arc<ModuleState> {
        &self.wiring
    }

    /// What this edge knows about `name` -- a UUID or an external id -- from
    /// whatever the server has sent: the wiring snapshot first (it names the
    /// kind and the profile), then the allow-list (a tool with no wiring is
    /// still a tool), then the compiled doors (a door whose wiring has not
    /// arrived). `None` when nothing knows it.
    pub fn resolve(&self, name: &str) -> Option<Resolved> {
        let uuid = Uuid::parse_str(name).ok();
        if let Some(w) = self.wiring.resource(name) {
            let kind = if w.kind == "door" {
                Kind::Door
            } else {
                Kind::Tool
            };
            let profile = SessionProfile::parse(&w.session_profile, kind);
            // A door's pulse is what the server compiled for it (its strike
            // binding's, or the site default); a tool's is its strike binding's
            // if it has one, which is how a latch on a non-door is pulsed.
            let pulse_ms = Uuid::parse_str(&w.id)
                .ok()
                .and_then(|id| self.doors.pulse_of(id))
                .or(w.strike_pulse_ms);
            return Some(Resolved {
                id: w.id,
                kind,
                profile,
                pulse_ms,
            });
        }
        if let Some(id) = self.allow_list.tool_uuid(name) {
            return Some(Resolved {
                id: id.to_string(),
                kind: Kind::Tool,
                profile: SessionProfile::UntilStopped,
                pulse_ms: None,
            });
        }
        if let Some(id) = uuid.filter(|id| self.doors.knows(*id)) {
            return Some(Resolved {
                id: id.to_string(),
                kind: Kind::Door,
                profile: SessionProfile::Momentary,
                pulse_ms: self.doors.pulse_of(id),
            });
        }
        None
    }

    /// The one decision for a badge at a resource, as of `now`.
    ///
    /// A power lockout refuses anything first: it is sticky and fail-secure,
    /// and it holds whether or not the server can be reached. Then the policy
    /// the resource has: compiled card lists for a door (deny beats the Open
    /// Access hold beats allow), the principal allow-list for a tool (card,
    /// member, authorization, availability -- in the order the reasons have
    /// always come out). A tool the server insists on deciding itself answers
    /// [`ScanDecision::AskServer`]. A name nothing knows is evaluated as a tool
    /// against the allow-list, which is what it always was, and answers
    /// `Unknown card` or `Tool not authorized for this user` as before.
    pub fn decide(&self, name: &str, card: &str, now: DateTime<Utc>) -> ScanDecision {
        if self.power.is_locked(name) {
            return ScanDecision::Deny("Tool is locked out (power)".to_string());
        }
        let resolved = self.resolve(name);
        let is_door = resolved.as_ref().is_some_and(|r| r.kind == Kind::Door);
        if is_door {
            let resolved = resolved.expect("checked");
            let door_id = Uuid::parse_str(&resolved.id).expect("a door is resolved by UUID");
            // The server ships door lists as card wire-digests, so match on the
            // digest of the swipe, never the raw code. No digester means an
            // empty token, which matches nothing: fail-closed.
            let token = self.allow_list.wire_digest_hex(card).unwrap_or_default();
            return match self.doors.decide_at(door_id, &token, now) {
                DoorDecision::Allow { duration_ms } => ScanDecision::Allow {
                    strike_ms: Some(duration_ms),
                },
                DoorDecision::Deny(why) => ScanDecision::Deny(why.to_string()),
            };
        }
        if self.allow_list.tool_requires_online(name) {
            return ScanDecision::AskServer;
        }
        match self.allow_list.check_access(card, name) {
            AccessResult::Authorized => ScanDecision::Allow {
                strike_ms: resolved.and_then(|r| r.pulse_ms),
            },
            AccessResult::UnknownCard => ScanDecision::Deny("Unknown card".to_string()),
            AccessResult::UserInactive => ScanDecision::Deny("User is not active".to_string()),
            AccessResult::ToolNotAuthorized => {
                ScanDecision::Deny("Tool not authorized for this user".to_string())
            }
            AccessResult::ToolUnavailable(s) => {
                ScanDecision::Deny(format!("Tool unavailable: {s}"))
            }
        }
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
        let held = self.doors.hold_pulses_at(now, hold_refresh);
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
    use crate::toolguard::{SyncTool, SyncUser};
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

    /// The one cache over the three ingests, as main.rs builds it: a door with a
    /// coordinator, a strike (4200 ms) and one power binding (the room), and a
    /// tool with a reader and a plug; the member's card opens both.
    fn cache(door_profile: &str) -> Arc<ResourceCache> {
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

        let doors = DoorsState::new();
        doors.apply_snapshot(DoorStateSnapshot {
            snapshot_at: t0(),
            doors: vec![CompiledDoor {
                id: door_id(),
                name: "Front".to_string(),
                enabled: true,
                unlock_duration_ms: 4200,
                allow_cards: vec![digest(CARD)],
                deny_cards: vec![],
                hold_unlock_until: None,
            }],
        });

        let allow_list = Arc::new(ToolGuardState::with_digester(Some(digester())));
        allow_list.apply_sync(SyncPayload {
            device_id: Uuid::from_u128(0xE),
            profile_field: "card".to_string(),
            tools: vec![
                SyncTool {
                    id: tool_id(),
                    external_id: Some("laser".to_string()),
                    name: "Laser".to_string(),
                    status: ToolStatus::Idle,
                    requires_online: false,
                },
                // Named by the allow-list and wired to nothing: still a tool.
                SyncTool {
                    id: Uuid::from_u128(0x7002),
                    external_id: Some("drill".to_string()),
                    name: "Drill".to_string(),
                    status: ToolStatus::Idle,
                    requires_online: false,
                },
            ],
            users: vec![SyncUser {
                profile_field_digest: digest(CARD),
                full_name: "M".to_string(),
                is_active: true,
                authorized_tool_ids: vec![tool_id()],
            }],
        });

        ResourceCache::new(wiring, doors, allow_list, Arc::new(PowerState::new()))
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

    #[test]
    fn a_door_and_a_tool_are_resolved_by_either_name_with_their_kind_and_profile() {
        let c = cache("");
        let door = c.resolve(&door_id().to_string()).expect("door");
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
            .expect("a tool the allow-list names and nothing wires");
        assert_eq!(unwired.kind, Kind::Tool);
        assert_eq!(unwired.id, Uuid::from_u128(0x7002).to_string());
        assert_eq!(unwired.pulse_ms, None);
        assert_eq!(
            cache("until_stopped")
                .resolve(&door_id().to_string())
                .unwrap()
                .profile,
            SessionProfile::UntilStopped,
            "the server's profile outranks the kind's default"
        );
    }

    #[test]
    fn one_decide_answers_a_door_with_its_pulse_and_a_tool_without_one() {
        let c = cache("");
        assert_eq!(
            c.decide(&door_id().to_string(), CARD, t0()),
            ScanDecision::Allow {
                strike_ms: Some(4200)
            }
        );
        assert_eq!(
            c.decide("laser", CARD, t0()),
            ScanDecision::Allow { strike_ms: None }
        );
        // The reasons are the ones each policy has always given.
        assert_eq!(
            c.decide(&door_id().to_string(), "NOBODY", t0()),
            ScanDecision::Deny("Card not authorized".to_string())
        );
        assert_eq!(
            c.decide("laser", "NOBODY", t0()),
            ScanDecision::Deny("Unknown card".to_string())
        );
        assert_eq!(
            c.decide("not-a-resource", CARD, t0()),
            ScanDecision::Deny("Tool not authorized for this user".to_string()),
            "a name nothing knows is judged as a tool, as it always was"
        );
    }

    #[test]
    fn a_momentary_door_session_leases_its_power_only_for_the_pulse() {
        let c = cache("");
        let ttl = Duration::seconds(3);
        let timeout = Duration::seconds(120);
        let refresh = Duration::seconds(30);
        let door = door_id().to_string();
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
        let door = door_id().to_string();
        c.open_session(&door, t0());
        let during = c.tick(t0() + Duration::seconds(1), ttl, timeout, refresh);
        assert_eq!(leases_for(&during, "dev-room-plug"), vec![true]);
        // Well past any pulse and any grace, and a sync that says nothing about
        // the door: still leased, because the server does not track a door's
        // session and this edge is the one that ends it.
        let sync = SyncPayload {
            device_id: Uuid::from_u128(0xE),
            profile_field: "card".to_string(),
            tools: vec![SyncTool {
                id: tool_id(),
                external_id: Some("laser".to_string()),
                name: "Laser".to_string(),
                status: ToolStatus::Idle,
                requires_online: false,
            }],
            users: vec![],
        };
        c.reconcile(&sync, t0() + Duration::minutes(10), None);
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
        let sync_idle = SyncPayload {
            device_id: Uuid::from_u128(0xE),
            profile_field: "card".to_string(),
            tools: vec![SyncTool {
                id: tool_id(),
                external_id: Some("laser".to_string()),
                name: "Laser".to_string(),
                status: ToolStatus::Idle,
                requires_online: false,
            }],
            users: vec![],
        };
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
        c.doors.apply_snapshot(DoorStateSnapshot {
            snapshot_at: t0() + Duration::seconds(1),
            doors: vec![CompiledDoor {
                id: door_id(),
                name: "Front".to_string(),
                enabled: true,
                unlock_duration_ms: 4200,
                allow_cards: vec![],
                deny_cards: vec![],
                hold_unlock_until: Some(t0() + Duration::minutes(10)),
            }],
        });
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
}
